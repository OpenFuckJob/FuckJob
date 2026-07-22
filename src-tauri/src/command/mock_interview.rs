use crate::command::base::CommandResult;
use crate::llm::service::LlmService;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockInterviewMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockInterviewQuestionRequest {
    pub session_id: String,
    pub resume_content: String,
    pub history: Vec<MockInterviewMessage>,
    pub round: u32,
    pub max_rounds: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockInterviewSummaryRequest {
    pub session_id: String,
    pub resume_content: String,
    pub history: Vec<MockInterviewMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MockInterviewStreamPayload {
    session_id: String,
    kind: String,
    content: String,
}

#[tauri::command]
pub async fn stream_mock_interview_question(
    app_handle: tauri::AppHandle,
    request: MockInterviewQuestionRequest,
) -> CommandResult<String> {
    if request.resume_content.trim().is_empty() {
        return CommandResult::err("请先输入/导入简历内容");
    }
    if request.round == 0 || request.round > request.max_rounds {
        return CommandResult::err("面试轮次不合法");
    }

    let prompt = build_question_prompt(&request);
    stream_prompt(app_handle, request.session_id, "question", prompt).await
}

#[tauri::command]
pub async fn stream_mock_interview_summary(
    app_handle: tauri::AppHandle,
    request: MockInterviewSummaryRequest,
) -> CommandResult<String> {
    if request.resume_content.trim().is_empty() {
        return CommandResult::err("请先输入/导入简历内容");
    }
    if request.history.is_empty() {
        return CommandResult::err("缺少模拟面试对话记录");
    }

    let prompt = build_summary_prompt(&request);
    stream_prompt(app_handle, request.session_id, "summary", prompt).await
}

async fn stream_prompt(
    app_handle: tauri::AppHandle,
    session_id: String,
    kind: &str,
    prompt: String,
) -> CommandResult<String> {
    let config = match crate::config::load_app_config_inner(app_handle.clone()) {
        Ok(value) => value,
        Err(error) => return CommandResult::err(error),
    };
    let credential = match crate::credential::resolve() {
        Ok(value) => value,
        Err(error) => return CommandResult::err(error),
    };
    let service = match LlmService::from_runtime(&config, &credential) {
        Ok(value) => value,
        Err(error) => return CommandResult::err(error),
    };
    let kind_text = kind.to_string();

    let result = service
        .stream(prompt, |delta| {
            app_handle
                .emit(
                    "mock_interview:delta",
                    MockInterviewStreamPayload {
                        session_id: session_id.clone(),
                        kind: kind_text.clone(),
                        content: delta,
                    },
                )
                .map_err(|error| {
                    crate::error::AppError::internal("无法发送流式事件")
                        .with_detail(error.to_string())
                })
        })
        .await;

    match result {
        Ok(response) => {
            let content = response.content;
            let _ = app_handle.emit(
                "mock_interview:done",
                MockInterviewStreamPayload {
                    session_id,
                    kind: kind.to_string(),
                    content: content.clone(),
                },
            );
            CommandResult::ok(content)
        }
        Err(error) => {
            let message = error.to_string();
            let _ = app_handle.emit(
                "mock_interview:error",
                MockInterviewStreamPayload {
                    session_id,
                    kind: kind.to_string(),
                    content: message.clone(),
                },
            );
            CommandResult::err(message)
        }
    }
}

fn build_question_prompt(request: &MockInterviewQuestionRequest) -> String {
    let focus = interview_focus(request.round);
    format!(
        r#"你是一位严格、专业、有压迫感但不失礼貌的技术面试官。你正在通过多轮追问帮助候选人暴露简历中的薄弱点，并挖掘可用于优化简历的真实事实。

候选人简历：
---
{resume_content}
---

历史对话：
{history}

当前轮次：{round}/{max_rounds}
本轮提问维度：{focus_name}
维度说明：{focus_description}

请只输出这一轮面试官要问的一个问题。
要求：
1. 问题必须基于简历或历史回答，并严格围绕“本轮提问维度”展开。
2. 五轮维度必须互不重复：第1轮技术深度，第2轮个人贡献，第3轮量化结果，第4轮问题处理，第5轮表达可信度。
3. 不要输出解释、编号、总结或多个问题。
4. 不要重复历史对话中已经问过的方向。
5. 如果当前维度下历史回答里有模糊表述，要继续追问可验证细节。"#,
        resume_content = request.resume_content,
        history = format_history(&request.history),
        round = request.round,
        max_rounds = request.max_rounds,
        focus_name = focus.name,
        focus_description = focus.description
    )
}

struct InterviewFocus {
    name: &'static str,
    description: &'static str,
}

fn interview_focus(round: u32) -> InterviewFocus {
    match round {
        1 => InterviewFocus {
            name: "技术深度",
            description: "围绕项目核心技术、实现细节、方案取舍、架构设计和技术难点提问。",
        },
        2 => InterviewFocus {
            name: "个人贡献",
            description: "追问候选人独立负责、主导设计、协作边界和本人实际产出。",
        },
        3 => InterviewFocus {
            name: "量化结果",
            description: "追问指标、收益、上线效果、业务影响、性能变化和可证明的数据。",
        },
        4 => InterviewFocus {
            name: "问题处理",
            description: "追问故障、瓶颈、踩坑、定位过程、解决方案和复盘改进。",
        },
        5 => InterviewFocus {
            name: "表达可信度",
            description: "追问简历表述真实性、上下文边界、证据链、复盘能力和表达一致性。",
        },
        _ => InterviewFocus {
            name: "表达可信度",
            description: "追问简历表述真实性、上下文边界、证据链、复盘能力和表达一致性。",
        },
    }
}

fn build_summary_prompt(request: &MockInterviewSummaryRequest) -> String {
    format!(
        r#"你是一位资深简历优化专家。请基于候选人的原始简历和完整模拟面试对话，完成一次最终总结和简历优化建议。

原始简历：
---
{resume_content}
---

完整对话：
{history}

请按以下结构输出 Markdown：
## 面试总结
- 按五个提问维度分别总结：技术深度、个人贡献、量化结果、问题处理、表达可信度。
- 每个维度都说明候选人在回答中暴露出的优势、薄弱点和可信事实。

## 可补充到简历的事实点
- 提取技术细节、行动步骤、量化数据、结果影响。

## 优化后的简历章节
请输出一个或多个可以直接替换进简历的二级章节，章节标题必须使用 `## 标题`。
不要输出客套话，不要包裹 Markdown 代码块。"#,
        resume_content = request.resume_content,
        history = format_history(&request.history)
    )
}

fn format_history(history: &[MockInterviewMessage]) -> String {
    if history.is_empty() {
        return "无".to_string();
    }

    history
        .iter()
        .map(|message| format!("{}：{}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        build_question_prompt, build_summary_prompt, MockInterviewMessage,
        MockInterviewQuestionRequest, MockInterviewSummaryRequest,
    };

    #[test]
    fn question_prompt_uses_round_and_history() {
        let request = MockInterviewQuestionRequest {
            session_id: "s1".to_string(),
            resume_content: "## 项目经历\n- 做过限流".to_string(),
            history: vec![MockInterviewMessage {
                role: "candidate".to_string(),
                content: "QPS 大约 3000".to_string(),
            }],
            round: 2,
            max_rounds: 5,
        };

        let prompt = build_question_prompt(&request);

        assert!(prompt.contains("当前轮次：2/5"));
        assert!(prompt.contains("QPS 大约 3000"));
        assert!(prompt.contains("请只输出这一轮面试官要问的一个问题"));
    }

    #[test]
    fn question_prompt_assigns_distinct_focus_by_round() {
        let expected_focus = [
            (1, "技术深度"),
            (2, "个人贡献"),
            (3, "量化结果"),
            (4, "问题处理"),
            (5, "表达可信度"),
        ];

        for (round, focus) in expected_focus {
            let request = MockInterviewQuestionRequest {
                session_id: "s1".to_string(),
                resume_content: "## 项目经历\n- 做过 RAG 检索".to_string(),
                history: vec![],
                round,
                max_rounds: 5,
            };

            let prompt = build_question_prompt(&request);

            assert!(prompt.contains(&format!("本轮提问维度：{focus}")));
            assert!(prompt.contains("五轮维度必须互不重复"));
        }
    }

    #[test]
    fn summary_prompt_requests_replaceable_markdown_sections() {
        let request = MockInterviewSummaryRequest {
            session_id: "s1".to_string(),
            resume_content: "## 项目经历\n- 做过限流".to_string(),
            history: vec![MockInterviewMessage {
                role: "interviewer".to_string(),
                content: "说说限流方案".to_string(),
            }],
        };

        let prompt = build_summary_prompt(&request);

        assert!(prompt.contains("## 面试总结"));
        assert!(prompt.contains("## 优化后的简历章节"));
        assert!(prompt.contains("章节标题必须使用 `## 标题`"));
    }

    #[test]
    fn summary_prompt_requires_five_focus_review() {
        let request = MockInterviewSummaryRequest {
            session_id: "s1".to_string(),
            resume_content: "## 项目经历\n- 做过 RAG 检索".to_string(),
            history: vec![MockInterviewMessage {
                role: "candidate".to_string(),
                content: "我负责混合检索".to_string(),
            }],
        };

        let prompt = build_summary_prompt(&request);

        assert!(prompt.contains("技术深度、个人贡献、量化结果、问题处理、表达可信度"));
        assert!(prompt.contains("按五个提问维度分别总结"));
    }
}
