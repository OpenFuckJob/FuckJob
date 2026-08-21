use crate::config::{AppRuntimeConfig, JobFilterConfig, MatchTarget, RuleMode};
use crate::rpa::common::RpaJob;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterDecision {
    pub matched: bool,
    pub reason: String,
}

impl FilterDecision {
    fn accept(reason: impl Into<String>) -> Self {
        Self {
            matched: true,
            reason: reason.into(),
        }
    }

    fn reject(reason: impl Into<String>) -> Self {
        Self {
            matched: false,
            reason: reason.into(),
        }
    }
}

fn normalized(value: &str) -> String {
    value.trim().to_lowercase()
}

fn configured_keywords(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| normalized(value))
        .filter(|value| !value.is_empty())
        .collect()
}

fn first_keyword_match(text: &str, keywords: &[String]) -> Option<String> {
    let text = normalized(text);
    configured_keywords(keywords)
        .into_iter()
        .find(|keyword| text.contains(keyword))
}

fn regex_target_text(job: &RpaJob, target: &MatchTarget) -> String {
    match target {
        MatchTarget::Title => job.title.clone(),
        MatchTarget::Company => job.company_name.clone(),
        MatchTarget::Description => job.detail.clone(),
        MatchTarget::All => format!("{}\n{}\n{}", job.title, job.company_name, job.detail),
    }
}

pub fn filter_decision(job: &RpaJob, config: &AppRuntimeConfig) -> FilterDecision {
    filter_decision_with_config(job, &config.job_filter_config)
}

fn filter_decision_with_config(job: &RpaJob, config: &JobFilterConfig) -> FilterDecision {
    if let Some(keyword) = first_keyword_match(&job.title, &config.exclude_keywords) {
        return FilterDecision::reject(format!("岗位标题命中排除关键词“{keyword}”"));
    }

    if let Some(keyword) = first_keyword_match(&job.company_name, &config.company_exclude_keywords)
    {
        return FilterDecision::reject(format!("公司名称命中排除关键词“{keyword}”"));
    }

    let title_keywords = configured_keywords(&config.keywords);
    if !title_keywords.is_empty()
        && !title_keywords
            .iter()
            .any(|keyword| normalized(&job.title).contains(keyword))
    {
        return FilterDecision::reject("岗位标题未命中任何包含关键词");
    }

    let company_keywords = configured_keywords(&config.company_keywords);
    if !company_keywords.is_empty()
        && !company_keywords
            .iter()
            .any(|keyword| normalized(&job.company_name).contains(keyword))
    {
        return FilterDecision::reject("公司名称未命中任何包含公司关键词");
    }

    let mut accept_rule_count = 0usize;
    let mut accept_rule_matched = false;
    for rule in &config.regex_rules {
        if rule.pattern.trim().is_empty() {
            continue;
        }
        let Ok(regex) = regex::Regex::new(&rule.pattern) else {
            if rule.mode == RuleMode::ACCEPT {
                accept_rule_count += 1;
            }
            continue;
        };
        let matches = regex.is_match(&regex_target_text(job, &rule.target));
        match rule.mode {
            RuleMode::REJECT if matches => {
                return FilterDecision::reject(format!("命中拒绝规则“{}”", rule.name));
            }
            RuleMode::ACCEPT => {
                accept_rule_count += 1;
                accept_rule_matched |= matches;
            }
            _ => {}
        }
    }

    if accept_rule_count > 0 && !accept_rule_matched {
        return FilterDecision::reject("未命中任何接受规则");
    }

    FilterDecision::accept("通过岗位规则筛选")
}

// 保留现有调用接口。
pub fn filter_verify(job: &RpaJob, config: &AppRuntimeConfig) -> bool {
    filter_decision(job, config).matched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_app_config, RegexRule};
    use crate::rpa::run_flow::PlatformKind;

    fn job(title: &str, company_name: &str, detail: &str) -> RpaJob {
        RpaJob {
            platform: PlatformKind::Boss,
            platform_job_id: "job-1".to_string(),
            title: title.to_string(),
            company_name: company_name.to_string(),
            detail: detail.to_string(),
            salary: "20-30K".to_string(),
            location: Some("南京".to_string()),
            recruiter_active_time: None,
            detail_url: "https://example.test/job-1".to_string(),
        }
    }

    #[test]
    fn keyword_matching_is_case_insensitive_and_ignores_blank_values() {
        let mut config = default_app_config();
        config.job_filter_config.keywords = vec!["  ".to_string(), "agent".to_string()];

        assert!(filter_verify(
            &job("AI Agent 开发工程师", "示例公司", ""),
            &config
        ));
        assert!(!filter_verify(&job("AI 产品经理", "示例公司", ""), &config));
    }

    #[test]
    fn company_include_keywords_are_enforced() {
        let mut config = default_app_config();
        config.job_filter_config.company_keywords = vec!["科技".to_string()];

        assert!(filter_verify(&job("Agent 开发", "示例科技", ""), &config));
        assert!(!filter_verify(&job("Agent 开发", "示例咨询", ""), &config));
    }

    #[test]
    fn any_accept_rule_may_match_but_at_least_one_is_required() {
        let mut config = default_app_config();
        config.job_filter_config.regex_rules = vec![
            RegexRule {
                name: "开发岗位".to_string(),
                pattern: "开发|工程师".to_string(),
                target: MatchTarget::Title,
                mode: RuleMode::ACCEPT,
            },
            RegexRule {
                name: "Agent 技术".to_string(),
                pattern: "Agent|RAG".to_string(),
                target: MatchTarget::Description,
                mode: RuleMode::ACCEPT,
            },
        ];

        assert!(filter_verify(
            &job("后端开发", "示例公司", "普通业务"),
            &config
        ));
        assert!(filter_verify(
            &job("技术顾问", "示例公司", "负责 Agent 平台"),
            &config
        ));
        assert!(!filter_verify(
            &job("产品经理", "示例公司", "负责产品规划"),
            &config
        ));
    }

    #[test]
    fn all_target_combines_title_company_and_description() {
        let mut config = default_app_config();
        config.job_filter_config.regex_rules = vec![RegexRule {
            name: "排除外包".to_string(),
            pattern: "外包".to_string(),
            target: MatchTarget::All,
            mode: RuleMode::REJECT,
        }];

        assert!(!filter_verify(
            &job("Agent 开发", "某外包公司", "负责平台开发"),
            &config
        ));
    }
}
