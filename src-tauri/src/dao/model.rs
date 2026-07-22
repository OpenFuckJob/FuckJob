use serde::{Deserialize, Serialize};

/// 岗位详情表
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JobDetail {
    /// 岗位唯一ID
    pub id: String,

    /// 岗位来源平台: boss / liepin
    #[serde(default)]
    pub platform: String,

    /// 岗位标题
    pub title: String,

    /// 公司名称
    pub company_name: String,

    /// 岗位描述（JD全文）
    pub detail: String,

    /// 薪资范围，例如：20k-40k·14薪
    pub salary: String,

    /// 工作地点
    pub location: Option<String>,

    /// 是否已与招聘方沟通/获得回复
    /// 默认 false
    pub is_reply: bool,

    /// 是否已投递简历
    /// 默认 false
    pub is_send_resume: bool,

    /// 创建时间（收藏或导入岗位时间）
    pub created_at: String,

    /// 投递时间
    /// 未投递则为 None
    pub resume_sent_at: Option<String>,

    /// 最后更新时间
    pub updated_at: String,
}

/// 岗位面试分析结果
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InterviewJobAnalysis {
    /// 关联岗位ID
    pub job_id: String,

    /// 分析时间
    pub analyzed_at: String,

    /// 总体匹配结论
    pub fit_summary: String,

    /// 匹配度评分（0~100）
    pub match_score: u8,

    /// 与岗位匹配的优势项
    pub strengths: Vec<String>,

    /// 风险项/短板项
    pub risks: Vec<String>,

    /// 技能匹配矩阵
    pub skill_matrix: Vec<SkillEvidence>,

    /// 预测面试问题
    pub likely_questions: Vec<InterviewQuestion>,

    /// 建议向面试官提问的问题
    pub questions_to_ask_interviewer: Vec<String>,

    /// 联网搜索摘要
    #[serde(default)]
    pub search_summary: String,

    /// 联网搜索来源
    #[serde(default)]
    pub search_sources: Vec<SearchSource>,

    /// 分析时使用的沟通上下文
    #[serde(default)]
    pub chat_context: String,

    /// LLM原始返回内容
    pub raw_response: String,

    /// 解析错误信息
    /// 解析成功则为 None
    pub parse_error: Option<String>,
}

/// 联网搜索来源
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SearchSource {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// 技能要求与简历证据映射
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SkillEvidence {
    /// JD中的技能要求
    pub requirement: String,

    /// 简历中的相关经历或证据
    pub resume_evidence: String,

    /// 能力差距分析
    pub gap: String,

    /// 面试前补强建议
    pub prep_action: String,
}

/// 面试问题预测
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InterviewQuestion {
    /// 问题类别
    /// 如：技术、项目经历、行为面试、系统设计等
    pub category: String,

    /// 面试问题
    pub question: String,

    /// 面试官提问意图
    pub why: String,

    /// 建议回答框架
    pub answer_outline: String,
}

/// 聊天消息持久化记录，按 jobId 关联
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatMessageRecord {
    /// 复合主键: "{job_id}:{mid}"
    pub id: String,
    pub job_id: String,
    pub mid: i64,
    /// true = 招聘者发送，false = 自己发送
    pub received: bool,
    pub text: String,
    /// 发送时间戳（毫秒）
    pub time: i64,
    pub from_name: String,
}
