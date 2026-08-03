use crate::command::base::CommandResult;
use crate::llm::service::LlmService;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

const MAX_QUESTION_CHARS: usize = 60;

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
    pub job_context: String,
    pub interview_type: String,
    pub difficulty: String,
    pub module_name: String,
    pub module_description: String,
    pub question_kind: String,
    pub focus_areas: Vec<String>,
    pub module_question: u32,
    pub module_target_questions: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockInterviewSummaryRequest {
    pub session_id: String,
    pub resume_content: String,
    pub history: Vec<MockInterviewMessage>,
    pub job_context: String,
    pub interview_type: String,
    pub difficulty: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MockInterviewReport {
    overall_score: u8,
    overall_summary: String,
    dimensions: Vec<MockInterviewDimension>,
    risks: Vec<String>,
    optimizations: Vec<MockResumeOptimization>,
    #[serde(default)]
    question_reviews: Vec<MockInterviewQuestionReview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MockInterviewQuestionReview {
    question_index: u32,
    question: String,
    answer: String,
    module: String,
    score: u8,
    summary: String,
    strengths: Vec<String>,
    improvements: Vec<String>,
    answer_outline: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MockInterviewDimension {
    dimension: String,
    score: u8,
    strengths: Vec<String>,
    weaknesses: Vec<String>,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MockResumeOptimization {
    section_title: String,
    original_markdown: String,
    optimized_markdown: String,
    rationale: String,
    evidence: Vec<String>,
    needs_evidence: bool,
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
    if request.round == 0 {
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
    let mut emitted_question_chars = 0usize;

    let result = service
        .stream(prompt, |delta| {
            let content = if kind_text == "question" {
                let remaining = MAX_QUESTION_CHARS.saturating_sub(emitted_question_chars);
                let value = delta.chars().take(remaining).collect::<String>();
                emitted_question_chars += value.chars().count();
                value
            } else {
                delta
            };
            if content.is_empty() {
                return Ok(());
            }
            app_handle
                .emit(
                    "mock_interview:delta",
                    MockInterviewStreamPayload {
                        session_id: session_id.clone(),
                        kind: kind_text.clone(),
                        content,
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
            let content = if kind == "summary" {
                match parse_interview_report(&response.content) {
                    Ok(report) => serde_json::to_string(&report).unwrap_or(response.content),
                    Err(error) => {
                        let message = error.to_string();
                        let _ = app_handle.emit(
                            "mock_interview:error",
                            MockInterviewStreamPayload {
                                session_id,
                                kind: kind.to_string(),
                                content: message,
                            },
                        );
                        return CommandResult::err(error);
                    }
                }
            } else {
                normalize_question(&response.content)
            };
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

目标岗位与 JD：
{job_context}

面试类型：{interview_type}
难度：{difficulty}
{resume_content}
---

历史对话：
{history}

当前问题序号：第 {round} 题
当前面试模块：{module_name}
模块目标：{module_description}
当前模块进度：第 {module_question}/{module_target_questions} 个核心问题
问题类型：{question_kind}
用户设置的侧重点：{focus_areas}
辅助考察维度：{focus_name}
维度说明：{focus_description}

请只输出这一轮面试官要问的一个问题。
要求：
1. 问题必须基于简历、目标岗位和历史回答，并围绕当前面试模块展开。
2. question_kind 为 followup 时必须承接上一条回答追问可验证细节；为 core 时提出该模块尚未覆盖的新问题。
3. 只问一个简短、口语化的问题，最多 60 个中文字符；不要堆叠多个子问题。
4. 不要输出解释、编号、总结，也不要重复历史对话中已经问过的问题。
5. 必须承接候选人上一轮回答；如果存在模糊、矛盾或缺少证据的表述，优先追问可验证细节。
6. 根据面试类型和难度控制问题深度，不要提出与目标岗位无关的问题。"#,
        resume_content = request.resume_content,
        job_context = fallback_context(&request.job_context),
        interview_type = request.interview_type,
        difficulty = request.difficulty,
        history = format_history(&request.history),
        round = request.round,
        focus_name = focus.name,
        focus_description = focus.description,
        module_name = request.module_name,
        module_description = request.module_description,
        question_kind = request.question_kind,
        focus_areas = if request.focus_areas.is_empty() {
            "AI 智能规划".to_string()
        } else {
            request.focus_areas.join("、")
        },
        module_question = request.module_question,
        module_target_questions = request.module_target_questions
    )
}

struct InterviewFocus {
    name: &'static str,
    description: &'static str,
}

fn interview_focus(round: u32) -> InterviewFocus {
    match ((round - 1) % 5) + 1 {
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
        _ => unreachable!(),
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

目标岗位与 JD：
{job_context}

面试类型：{interview_type}
难度：{difficulty}

只输出合法 JSON 对象，不要输出 Markdown 代码块、前言或解释。格式必须为：
{{
  "overallScore": 0到100的整数,
  "overallSummary": "总体评价",
  "dimensions": [
    {{
      "dimension": "技术深度",
      "score": 0到100的整数,
      "strengths": ["优势"],
      "weaknesses": ["薄弱点"],
      "evidence": ["来自对话的事实依据"]
    }}
  ],
  "risks": ["真实性、岗位匹配或表达风险"],
  "optimizations": [],
  "questionReviews": [
    {{
      "questionIndex": 1,
      "question": "面试官问题原文",
      "answer": "候选人回答原文",
      "module": "所属面试模块",
      "score": 0到100的整数,
      "summary": "本题评价",
      "strengths": ["做得好的部分"],
      "improvements": ["可以改进的部分"],
      "answerOutline": ["基于真实经历的建议回答结构，不编造示范答案"]
    }}
  ]
}}

要求：
1. dimensions 必须覆盖技术深度、个人贡献、量化结果、问题处理、表达可信度。
2. optimizations 固定返回空数组，本报告不生成或修改简历。
3. questionReviews 必须覆盖所有已回答或跳过的核心问题，追问可合并到对应核心问题。
4. 不得编造经历或数据；每项评分和评价必须能追溯到对话原文。
5. 未充分考察的能力要在评价中明确说明，不能直接给低分。"#,
        resume_content = request.resume_content,
        history = format_history(&request.history),
        job_context = fallback_context(&request.job_context),
        interview_type = request.interview_type,
        difficulty = request.difficulty
    )
}

fn fallback_context(value: &str) -> &str {
    if value.trim().is_empty() {
        "未提供，基于简历进行通用技术面试"
    } else {
        value
    }
}

fn normalize_question(raw: &str) -> String {
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let first_question = compact
        .char_indices()
        .find(|(_, value)| matches!(value, '？' | '?'))
        .map(|(index, value)| compact[..index + value.len_utf8()].to_string())
        .unwrap_or(compact);
    if first_question.chars().count() <= MAX_QUESTION_CHARS {
        return first_question;
    }
    let mut shortened = first_question
        .chars()
        .take(MAX_QUESTION_CHARS - 1)
        .collect::<String>();
    shortened.push('…');
    shortened
}

fn parse_interview_report(raw: &str) -> Result<MockInterviewReport, crate::error::AppError> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let report: MockInterviewReport = serde_json::from_str(cleaned).map_err(|error| {
        crate::error::AppError::validation("模拟面试报告格式无效").with_detail(error.to_string())
    })?;
    if report.overall_score > 100 || report.dimensions.is_empty() {
        return Err(crate::error::AppError::validation(
            "模拟面试报告评分或维度无效",
        ));
    }
    if report.dimensions.iter().any(|item| item.score > 100) {
        return Err(crate::error::AppError::validation(
            "模拟面试报告维度评分无效",
        ));
    }
    if report.optimizations.len() > 3 {
        return Err(crate::error::AppError::validation(
            "模拟面试生成的简历优化项过多",
        ));
    }
    Ok(report)
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
        build_question_prompt, build_summary_prompt, normalize_question, parse_interview_report,
        MockInterviewMessage, MockInterviewQuestionRequest, MockInterviewSummaryRequest,
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
            job_context: "高级 Java 后端，要求高并发".to_string(),
            interview_type: "技术面".to_string(),
            difficulty: "高级".to_string(),
            module_name: "项目深挖".to_string(),
            module_description: "项目事实与个人贡献".to_string(),
            question_kind: "followup".to_string(),
            focus_areas: vec!["高并发".to_string()],
            module_question: 2,
            module_target_questions: 4,
        };

        let prompt = build_question_prompt(&request);

        assert!(prompt.contains("当前问题序号：第 2 题"));
        assert!(prompt.contains("当前面试模块：项目深挖"));
        assert!(prompt.contains("QPS 大约 3000"));
        assert!(prompt.contains("高级 Java 后端"));
        assert!(prompt.contains("必须承接候选人上一轮回答"));
        assert!(prompt.contains("最多 60 个中文字符"));
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
                job_context: String::new(),
                interview_type: "技术面".to_string(),
                difficulty: "中级".to_string(),
                module_name: "专业能力".to_string(),
                module_description: "岗位核心专业能力".to_string(),
                question_kind: "core".to_string(),
                focus_areas: vec![],
                module_question: 1,
                module_target_questions: 3,
            };

            let prompt = build_question_prompt(&request);

            assert!(prompt.contains(&format!("辅助考察维度：{focus}")));
            assert!(prompt.contains("当前面试模块：专业能力"));
        }

        let sixth_round = MockInterviewQuestionRequest {
            session_id: "s2".to_string(),
            resume_content: "## 项目经历\n- 做过 RAG 检索".to_string(),
            history: vec![],
            round: 6,
            job_context: String::new(),
            interview_type: "技术面".to_string(),
            difficulty: "中级".to_string(),
            module_name: "专业能力".to_string(),
            module_description: "岗位核心专业能力".to_string(),
            question_kind: "core".to_string(),
            focus_areas: vec![],
            module_question: 1,
            module_target_questions: 3,
        };
        assert!(build_question_prompt(&sixth_round).contains("辅助考察维度：技术深度"));
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
            job_context: "后端研发".to_string(),
            interview_type: "综合面".to_string(),
            difficulty: "高级".to_string(),
        };

        let prompt = build_summary_prompt(&request);

        assert!(prompt.contains("\"overallScore\""));
        assert!(prompt.contains("\"optimizations\""));
        assert!(prompt.contains("\"questionReviews\""));
        assert!(prompt.contains("不得编造经历或数据"));
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
            job_context: String::new(),
            interview_type: "技术面".to_string(),
            difficulty: "中级".to_string(),
        };

        let prompt = build_summary_prompt(&request);

        assert!(prompt.contains("技术深度、个人贡献、量化结果、问题处理、表达可信度"));
        assert!(prompt.contains("dimensions 必须覆盖"));
    }

    #[test]
    fn structured_report_is_validated() {
        let report = parse_interview_report(r###"```json
{
  "overallScore": 82,
  "overallSummary": "技术基础扎实",
  "dimensions": [{"dimension":"技术深度","score":85,"strengths":["方案清晰"],"weaknesses":[],"evidence":["解释了限流取舍"]}],
  "risks": [],
  "optimizations": [{"sectionTitle":"项目经历","originalMarkdown":"## 项目经历\\n- 做过限流","optimizedMarkdown":"## 项目经历\\n- 设计限流方案","rationale":"补充个人贡献","evidence":["本人负责设计"],"needsEvidence":false}]
}
```"###).unwrap();

        assert_eq!(report.overall_score, 82);
        assert_eq!(report.optimizations.len(), 1);
    }

    #[test]
    fn generated_question_keeps_only_one_short_question() {
        let normalized = normalize_question(
            "请先说明你在项目中的具体职责？另外再详细介绍技术方案、性能数据以及遇到的问题。",
        );
        assert_eq!(normalized, "请先说明你在项目中的具体职责？");

        let long = normalize_question(&format!("{}？", "很长的问题".repeat(30)));
        assert!(long.chars().count() <= 60);
    }
}
