//! 统一的 Agent 执行循环。
//!
//! 在这之前，每个用到大模型的地方都自己写一遍「装参数 → 渲染提示词 → 调用 → 解析 → 降级」，
//! 于是同一类错误反复出现：猎聘回复因为漏填 `job_description` 触发渲染报错，
//! 一路静默降级成「未匹配到回复模板」，实际效果是自动回复整个平台都不发消息。
//!
//! 这里把那条链路收成一处，调用方只需要描述任务本身：给什么上下文、
//! 期望什么结构、什么样的结果算合格。装参数、渲染、重试、净化、校验由循环负责。

use std::time::Instant;

use serde_json::Value;

use crate::agent::output;
use crate::agent::trace::{self, AgentTrace, RoundTrace, RoundVerdict};
use crate::config::AppRuntimeConfig;
use crate::error::AppError;
use crate::llm::service::LlmChainService;
use crate::llm::template;
use crate::logger;

/// 校验不通过时追加给模型的返工说明。
///
/// 单独成段并重申输出要求，是因为多数模型在长提示词末尾的指令上更听话；
/// 直接把原提示词重发一遍则会让模型倾向于复述上一次的错误答案。
const RETRY_HEADER: &str = "\n\n---\n【返工要求】\n你上一次的输出没有通过校验，原因：";
const RETRY_FOOTER: &str =
    "\n请针对上述原因重新输出，只给出最终结果本身，不要解释、不要道歉、不要复述本要求。";

/// 一次运行的结束方式。区分开是为了让调用方能把「需要返工才成功」记进日志——
/// 这类信号积累起来说明提示词该改了，而不是模型不稳定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStop {
    /// 首轮直接通过
    FirstTry,
    /// 靠返工救回来的
    Recovered,
}

impl AgentStop {
    /// 轨迹里存字符串而不是直接序列化枚举：这个值要送到前端展示，
    /// 存成稳定的小写标识后，以后给枚举加分支也不会改掉已有轨迹的字面量
    fn trace_label(self) -> &'static str {
        match self {
            Self::FirstTry => "first_try",
            Self::Recovered => "recovered",
        }
    }
}

/// 一次运行的完整结果。
#[derive(Debug, Clone)]
pub struct AgentOutcome<T> {
    pub output: T,
    pub stop: AgentStop,
    /// 实际消耗的轮次，从 1 起算
    pub rounds: u32,
    /// 净化后的最后一次模型输出。落库后才能回答「当初为什么发了这句话」
    pub raw: String,
    /// 被否掉的轮次理由，按顺序。为空表示一次过
    pub rejections: Vec<String>,
    /// 本次运行在轨迹缓冲里的 id，供测试模式关联展示
    pub trace_id: String,
}

/// 一个可执行的 Agent 任务。
///
/// 实现者只描述任务，不碰模型调用：这样新增一种用途不会再复制一遍调用样板，
/// 也就不会再漏掉净化或者填参数。
pub trait AgentTask {
    /// 结构化结果类型。纯文本任务用 `String`
    type Output;

    /// 任务名，只用于日志
    fn name(&self) -> &'static str;

    /// 提示词模板。用户可配的任务从配置里取，内置任务返回固定字符串
    fn prompt_template(&self) -> Result<String, AppError>;

    /// 装配模板变量。
    ///
    /// **必须把模板可能引用的变量全部填上**，哪怕值为空也要显式填占位说明。
    /// [`crate::llm::template::render`] 对缺失变量是直接报错的，漏一个就是整条链路失效，
    /// 而失败点在渲染阶段，日志上看起来却像「模型没生成内容」，极难排查。
    fn params(&self) -> Result<Value, AppError>;

    /// 组装最终提示词。默认就是「取模板 + 填变量 + 渲染」。
    ///
    /// 需要把用户自定义模板嵌进内置外壳的任务（例如回复决策）可以覆写这里：
    /// 用户模板必须先单独渲染完，再拼进外壳，否则用户正文里的花括号会被
    /// 外层渲染当成占位符而报错。
    fn build_prompt(&self) -> Result<String, AppError> {
        template::render(&self.prompt_template()?, &self.params()?)
    }

    /// 把模型输出解析成结果。传入的 `raw` 已经过 [`output::sanitize`]。
    ///
    /// 返回 `Err` 的字符串会原样作为返工理由发回给模型，所以要写成模型看得懂的人话。
    fn parse(&self, raw: &str) -> Result<Self::Output, String>;

    /// 结果体检。解析得出来不等于能用——空内容、占位符、拒答话术都要在这里拦下。
    fn validate(&self, _output: &Self::Output) -> Result<(), String> {
        Ok(())
    }

    /// 含首轮在内的最大轮次。默认给一次返工机会：
    /// 两轮还不合格通常是提示词或模型能力问题，再试只是浪费额度。
    fn max_rounds(&self) -> u32 {
        2
    }
}

/// 带可选取消检查的执行器。
///
/// 取消检查做成注入的而不是直接读 RPA 的全局标志：命令层（简历优化、规则生成等）
/// 并不属于任何自动化任务，读那个标志会被上一次已停止的任务误伤。
pub struct AgentRunner<'a> {
    config: &'a AppRuntimeConfig,
    cancel: Option<Box<dyn Fn() -> bool + Send + Sync + 'a>>,
    trace_id: Option<String>,
}

impl<'a> AgentRunner<'a> {
    pub fn new(config: &'a AppRuntimeConfig) -> Self {
        Self {
            config,
            cancel: None,
            trace_id: None,
        }
    }

    /// 注册取消检查。每轮模型调用前后各查一次，停止请求下来时不再浪费额度
    pub fn with_cancel(mut self, check: impl Fn() -> bool + Send + Sync + 'a) -> Self {
        self.cancel = Some(Box::new(check));
        self
    }

    /// 指定这次运行的轨迹 id，让调用方在运行之前就拿到它。
    ///
    /// 失败时 `execute` 只能返回 [`AppError`]，`AgentOutcome` 连同里面的 `trace_id`
    /// 一起没有了——而失败恰恰是调提示词时最需要翻轨迹的时候。调用方靠「取最新一条」
    /// 反查在并发跑任务时会认错人，所以把 id 的分配权交给它：
    /// 先 [`trace::next_id`] 拿号，再传进来，成功失败都能对上。
    pub fn with_trace_id(mut self, id: impl Into<String>) -> Self {
        self.trace_id = Some(id.into());
        self
    }

    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|check| check())
    }

    pub async fn run<T: AgentTask>(&self, task: &T) -> Result<AgentOutcome<T::Output>, AppError> {
        self.execute(task, None::<fn(String) -> Result<(), AppError>>)
            .await
    }

    /// 边生成边把增量推给调用方（模拟面试这类需要实时展示的用途）。
    ///
    /// 流式下**强制单轮**：增量已经落到界面上了，返工重发会让用户看到重复内容。
    /// 这条约束写在循环里而不是「这个用途特殊、绕开循环自己实现」——
    /// 后者正是此前流式调用游离在体系之外、拿不到统一取消与日志的原因。
    pub async fn run_streaming<T, F>(
        &self,
        task: &T,
        on_delta: F,
    ) -> Result<AgentOutcome<T::Output>, AppError>
    where
        T: AgentTask,
        F: FnMut(String) -> Result<(), AppError>,
    {
        self.execute(task, Some(on_delta)).await
    }

    async fn execute<T, F>(
        &self,
        task: &T,
        mut on_delta: Option<F>,
    ) -> Result<AgentOutcome<T::Output>, AppError>
    where
        T: AgentTask,
        F: FnMut(String) -> Result<(), AppError>,
    {
        // 埋点在这里而不是各个调用方：这是全部八种模型用途唯一的出口，
        // 埋一处就等于全覆盖，也不会有新用途忘了接
        let trace_id = self
            .trace_id
            .clone()
            .unwrap_or_else(trace::next_id);
        let started_at = chrono::Local::now().to_rfc3339();
        let started = Instant::now();
        let mut trace_rounds: Vec<RoundTrace> = Vec::new();

        // 准备阶段的两次失败也要留痕，哪怕一轮都还没跑起来。
        // 模板漏填变量正是这个项目踩过最深的坑（猎聘自动回复整个平台静默失效，
        // 见本文件末尾的回归测试），而它的报错发生在这里、轨迹却什么都没有的话，
        // 测试模式页面就会在最该看清的地方一片空白
        let service = match LlmChainService::from_runtime(self.config) {
            Ok(service) => service,
            Err(error) => {
                record_trace(
                    &trace_id,
                    task.name(),
                    &started_at,
                    started,
                    trace_rounds,
                    None,
                    Some(error.message.clone()),
                );
                return Err(error);
            }
        };
        let base_prompt = match task.build_prompt() {
            Ok(prompt) => prompt,
            Err(error) => {
                record_trace(
                    &trace_id,
                    task.name(),
                    &started_at,
                    started,
                    trace_rounds,
                    None,
                    Some(error.message.clone()),
                );
                return Err(error);
            }
        };

        let streaming = on_delta.is_some();
        let max_rounds = if streaming {
            1
        } else {
            task.max_rounds().max(1)
        };
        let mut rejections: Vec<String> = Vec::new();

        for round in 1..=max_rounds {
            if self.cancelled() {
                let error = AppError::cancelled("任务已停止，大模型调用已取消");
                record_trace(
                    &trace_id,
                    task.name(),
                    &started_at,
                    started,
                    trace_rounds,
                    None,
                    Some(error.message.clone()),
                );
                return Err(error);
            }

            let prompt = match rejections.last() {
                None => base_prompt.clone(),
                Some(reason) => format!("{base_prompt}{RETRY_HEADER}{reason}{RETRY_FOOTER}"),
            };

            // 两条路都是流式：流式避开了网关掐断静默长连接、整体超时把已生成内容
            // 全部作废这两个坑。区别只在增量推不推出去——不推才敢重试和降级
            let call_started = Instant::now();
            let called = match on_delta.as_mut() {
                Some(callback) => service.stream_with(prompt.clone(), callback).await,
                None => service.stream_collect(prompt.clone()).await,
            };
            let call_ms = elapsed_ms(call_started);

            // 原先这里是直接 `?` 抛出的，失败的那一轮什么都留不下——
            // 而「调用压根没成功」恰恰是调提示词时最需要区分的一种情况，
            // 所以先把这一轮连同整条轨迹落进缓冲，再原样把错误抛出去
            let response = match called {
                Ok(response) => response,
                Err(error) => {
                    // 只记 message：detail 里可能带上游返回体，含鉴权信息
                    trace_rounds.push(RoundTrace {
                        round,
                        prompt,
                        raw: String::new(),
                        model: None,
                        usage: None,
                        duration_ms: call_ms,
                        verdict: RoundVerdict::Failed {
                            reason: error.message.clone(),
                        },
                    });
                    record_trace(
                        &trace_id,
                        task.name(),
                        &started_at,
                        started,
                        trace_rounds,
                        None,
                        Some(error.message.clone()),
                    );
                    return Err(error);
                }
            };

            if self.cancelled() {
                // 这一轮拿到了输出却没走到解析，三种 verdict 没有一个描述得准，
                // 与其把 Rejected 的语义弄脏，不如让它缺席，由整条轨迹的 error 说明
                let error = AppError::cancelled("任务已停止，大模型调用已取消");
                record_trace(
                    &trace_id,
                    task.name(),
                    &started_at,
                    started,
                    trace_rounds,
                    None,
                    Some(format!("{}（末轮输出未参与解析）", error.message)),
                );
                return Err(error);
            }

            let raw = output::sanitize(&response.content);
            let model = response.model.clone();
            let usage = response.usage.clone();
            let rejection = match task.parse(&raw) {
                Err(reason) => reason,
                Ok(parsed) => match task.validate(&parsed) {
                    Ok(()) => {
                        let stop = if rejections.is_empty() {
                            AgentStop::FirstTry
                        } else {
                            AgentStop::Recovered
                        };
                        trace_rounds.push(RoundTrace {
                            round,
                            prompt,
                            raw: raw.clone(),
                            model,
                            usage,
                            duration_ms: call_ms,
                            verdict: RoundVerdict::Passed,
                        });
                        record_trace(
                            &trace_id,
                            task.name(),
                            &started_at,
                            started,
                            trace_rounds,
                            Some(stop.trace_label()),
                            None,
                        );
                        return Ok(AgentOutcome {
                            output: parsed,
                            stop,
                            rounds: round,
                            raw,
                            rejections,
                            trace_id,
                        });
                    }
                    Err(reason) => reason,
                },
            };

            trace_rounds.push(RoundTrace {
                round,
                prompt,
                raw: raw.clone(),
                model,
                usage,
                duration_ms: call_ms,
                verdict: RoundVerdict::Rejected {
                    reason: rejection.clone(),
                },
            });

            // 内容本身可能含求职者隐私，日志只留原因和长度
            let _ = logger::warning(format!(
                "Agent「{}」第 {}/{} 轮输出未通过校验（{} 字）：{}",
                task.name(),
                round,
                max_rounds,
                raw.chars().count(),
                rejection
            ));
            rejections.push(rejection);
        }

        let error = AppError::provider(format!(
            "Agent「{}」连续 {} 轮输出都不合格：{}",
            task.name(),
            max_rounds,
            rejections.last().map(String::as_str).unwrap_or("原因未知")
        ));
        record_trace(
            &trace_id,
            task.name(),
            &started_at,
            started,
            trace_rounds,
            None,
            Some(error.message.clone()),
        );
        Err(error)
    }
}

fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// 把一次运行落进内存缓冲。
///
/// 每条退出路径上各调一次，而不是用 Drop 守卫自动兜底：守卫只知道「函数结束了」，
/// 拿不到「成功还是失败、失败在哪一步」这些只有退出点才清楚的信息，
/// 而这恰恰是测试模式要看的东西。代价是新增退出路径时得记得补一行。
fn record_trace(
    id: &str,
    task_name: &str,
    started_at: &str,
    started: Instant,
    rounds: Vec<RoundTrace>,
    stop: Option<&str>,
    error: Option<String>,
) {
    trace::record(AgentTrace {
        id: id.to_string(),
        task_name: task_name.to_string(),
        started_at: started_at.to_string(),
        duration_ms: elapsed_ms(started),
        rounds,
        stop: stop.map(str::to_string),
        error,
    });
}

/// 不需要取消检查时的快捷入口
pub async fn run<T: AgentTask>(
    task: &T,
    config: &AppRuntimeConfig,
) -> Result<AgentOutcome<T::Output>, AppError> {
    AgentRunner::new(config).run(task).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 只验证不依赖网络的那部分：变量装配、返工提示词拼接、轮次上限。
    /// 真正的模型调用由 `LlmChainService` 自己的测试覆盖。
    struct FakeTask {
        template: &'static str,
        params: Value,
    }

    impl AgentTask for FakeTask {
        type Output = String;

        fn name(&self) -> &'static str {
            "fake"
        }

        fn prompt_template(&self) -> Result<String, AppError> {
            Ok(self.template.to_string())
        }

        fn params(&self) -> Result<Value, AppError> {
            Ok(self.params.clone())
        }

        fn parse(&self, raw: &str) -> Result<Self::Output, String> {
            Ok(raw.to_string())
        }
    }

    #[test]
    fn missing_template_variable_fails_at_render_not_silently() {
        // 这是猎聘自动回复全平台失效的根因：模板引用了 job_description，
        // 装参数时却只在查得到岗位时才填，查不到就整条链路报错
        let task = FakeTask {
            template: "岗位：{{job_description}} 历史：{{chat_history}}",
            params: json!({ "chat_history": "你好" }),
        };

        let params = task.params().unwrap();
        let error = template::render(&task.prompt_template().unwrap(), &params).unwrap_err();

        assert!(error.message.contains("job_description"));
    }

    #[test]
    fn fully_populated_variables_render_even_when_values_are_empty() {
        let task = FakeTask {
            template: "岗位：{{job_description}} 历史：{{chat_history}}",
            params: json!({ "job_description": "（未获取到岗位描述）", "chat_history": "你好" }),
        };

        let prompt = template::render(&task.prompt_template().unwrap(), &task.params().unwrap())
            .expect("变量填全后必须能渲染");

        assert!(prompt.contains("（未获取到岗位描述）"));
    }

    #[test]
    fn retry_prompt_states_the_reason_and_forbids_apologising() {
        let base = "原始提示词";
        let prompt = format!("{base}{RETRY_HEADER}输出不是合法 JSON{RETRY_FOOTER}");

        assert!(prompt.starts_with(base));
        assert!(prompt.contains("输出不是合法 JSON"));
        assert!(prompt.contains("不要解释、不要道歉"));
    }

    #[test]
    fn default_max_rounds_gives_exactly_one_retry() {
        let task = FakeTask {
            template: "x",
            params: json!({}),
        };

        assert_eq!(task.max_rounds(), 2);
    }

    /// 「返工一次才通过」的完整轨迹（第 1 轮 Rejected、第 2 轮 Passed）这里测不了：
    /// 走完 `execute` 至少要两次真实的模型往返，而这组测试的前提就是不发网络请求，
    /// 为了可测把循环拆成可注入的假 service，等于为测试重塑生产结构，代价大于收益。
    /// 因此这里只守住埋点的形状（每条退出路径都带上 trace_id），
    /// 缓冲本身的行为由 `agent::trace` 的单元测试覆盖，两轮串起来的效果靠集成验证。
    #[test]
    fn outcome_carries_a_trace_id_for_the_test_mode_page() {
        let outcome = AgentOutcome {
            output: "结果".to_string(),
            stop: AgentStop::Recovered,
            rounds: 2,
            raw: "结果".to_string(),
            rejections: vec!["输出不是合法 JSON".to_string()],
            trace_id: "trace-7".to_string(),
        };

        assert_eq!(outcome.trace_id, "trace-7");
        assert_eq!(AgentStop::FirstTry.trace_label(), "first_try");
        assert_eq!(AgentStop::Recovered.trace_label(), "recovered");
    }
}
