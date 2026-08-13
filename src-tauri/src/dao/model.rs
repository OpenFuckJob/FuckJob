use serde::{Deserialize, Serialize};

use crate::config::{
    AppRuntimeConfig, GreetConfig, JobFilterConfig, PlatformFilterConfig, ReplayConfig,
    ResumeConfig,
};

/// 一次求职任务实际使用的不可变方案内容。
///
/// 这里只保存方案拥有的五块策略，不复制浏览器、模型提供商等全局配置。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct JobProfileSnapshot {
    pub snapshot_id: String,
    pub profile_id: String,
    pub profile_name: String,
    pub job_filter_config: JobFilterConfig,
    pub platform_filter_config: PlatformFilterConfig,
    pub greet_config: GreetConfig,
    pub replay_config: ReplayConfig,
    pub resume_config: ResumeConfig,
}

impl JobProfileSnapshot {
    pub fn from_resolved(config: &AppRuntimeConfig) -> Option<Self> {
        let active = config.active_job_profile.as_ref()?;
        Some(Self {
            snapshot_id: active.snapshot_id.clone(),
            profile_id: active.id.clone(),
            profile_name: active.name.clone(),
            job_filter_config: config.job_filter_config.clone(),
            platform_filter_config: config.platform_filter_config.clone(),
            greet_config: config.greet_config.clone(),
            replay_config: config.replay_config.clone(),
            resume_config: config.resume_config.clone(),
        })
    }

    pub fn apply_to(&self, base: &AppRuntimeConfig) -> AppRuntimeConfig {
        let mut config = base.clone();
        config.job_filter_config = self.job_filter_config.clone();
        config.platform_filter_config = self.platform_filter_config.clone();
        config.greet_config = self.greet_config.clone();
        config.replay_config = self.replay_config.clone();
        config.resume_config = self.resume_config.clone();
        config.active_job_profile = Some(crate::config::ActiveJobProfile {
            id: self.profile_id.clone(),
            name: self.profile_name.clone(),
            snapshot_id: self.snapshot_id.clone(),
        });
        config
    }
}

/// 岗位详情表
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JobDetail {
    /// 岗位唯一ID
    pub id: String,

    /// 岗位来源平台: boss / liepin
    #[serde(default)]
    pub platform: String,

    /// 首次建联任务。旧数据没有该字段时保持为空。
    #[serde(default)]
    pub source_task_id: Option<String>,

    /// 首次建联使用的求职方案。
    #[serde(default)]
    pub profile_id: Option<String>,

    /// 冗余保存方案名称，便于历史任务和岗位直接展示。
    #[serde(default)]
    pub profile_name: Option<String>,

    /// 首次建联时不可变方案快照的标识。
    #[serde(default)]
    pub profile_snapshot_id: Option<String>,

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_app_config, resolve_job_profile};

    #[test]
    fn snapshot_restores_original_profile_strategy_after_base_changes() {
        let base = default_app_config();
        let resolved = resolve_job_profile(&base, None).unwrap();
        let snapshot = JobProfileSnapshot::from_resolved(&resolved.config).unwrap();
        let original_query = snapshot.job_filter_config.query.clone();
        let original_prompt = snapshot.greet_config.reply_prompt.clone();

        let mut edited = base;
        edited.job_filter_config.query = Some("完全不同的岗位".into());
        edited.greet_config.reply_prompt = Some("已经修改的新提示词".into());
        let restored = snapshot.apply_to(&edited);

        assert_eq!(restored.job_filter_config.query, original_query);
        assert_eq!(restored.greet_config.reply_prompt, original_prompt);
        assert_eq!(
            restored
                .active_job_profile
                .as_ref()
                .map(|value| value.snapshot_id.as_str()),
            Some(snapshot.snapshot_id.as_str())
        );
    }
}
