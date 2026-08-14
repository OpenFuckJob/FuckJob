//! 统一的 Agent 执行循环。
//!
//! 在这之前，每个用到大模型的地方都自己写一遍「装参数 → 渲染提示词 → 调用 → 解析 → 降级」，
//! 于是同一类错误反复出现：猎聘回复因为漏填 `job_description` 触发渲染报错，
//! 一路静默降级成「未匹配到回复模板」，实际效果是自动回复整个平台都不发消息。
//!
//! 这里把那条链路收成一处，调用方只需要描述任务本身：给什么上下文、
//! 期望什么结构、什么样的结果算合格。装参数、渲染、重试、净化、校验由循环负责。

use serde_json::Value;

use crate::agent::output;
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
}

impl<'a> AgentRunner<'a> {
    pub fn new(config: &'a AppRuntimeConfig) -> Self {
        Self {
            config,
            cancel: None,
        }
    }

    /// 注册取消检查。每轮模型调用前后各查一次，停止请求下来时不再浪费额度
    pub fn with_cancel(mut self, check: impl Fn() -> bool + Send + Sync + 'a) -> Self {
        self.cancel = Some(Box::new(check));
        self
    }

    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|check| check())
    }

    pub async fn run<T: AgentTask>(&self, task: &T) -> Result<AgentOutcome<T::Output>, AppError> {
        let service = LlmChainService::from_runtime(self.config)?;
        let base_prompt = task.build_prompt()?;

        let max_rounds = task.max_rounds().max(1);
        let mut rejections: Vec<String> = Vec::new();

        for round in 1..=max_rounds {
            if self.cancelled() {
                return Err(AppError::cancelled("任务已停止，大模型调用已取消"));
            }

            let prompt = match rejections.last() {
                None => base_prompt.clone(),
                Some(reason) => format!("{base_prompt}{RETRY_HEADER}{reason}{RETRY_FOOTER}"),
            };

            // 走流式采集而非一次性返回：增量全在内部累积、不外推，因此可以安全重试，
            // 同时避开网关掐断静默长连接、整体超时把已生成内容全部作废这两个坑
            let response = service.stream_collect(prompt).await?;

            if self.cancelled() {
                return Err(AppError::cancelled("任务已停止，大模型调用已取消"));
            }

            let raw = output::sanitize(&response.content);
            let rejection = match task.parse(&raw) {
                Err(reason) => reason,
                Ok(parsed) => match task.validate(&parsed) {
                    Ok(()) => {
                        return Ok(AgentOutcome {
                            output: parsed,
                            stop: if rejections.is_empty() {
                                AgentStop::FirstTry
                            } else {
                                AgentStop::Recovered
                            },
                            rounds: round,
                            raw,
                            rejections,
                        });
                    }
                    Err(reason) => reason,
                },
            };

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

        Err(AppError::provider(format!(
            "Agent「{}」连续 {} 轮输出都不合格：{}",
            task.name(),
            max_rounds,
            rejections.last().map(String::as_str).unwrap_or("原因未知")
        )))
    }
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
}
