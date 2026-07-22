use crate::config::{AppRuntimeConfig, MatchTarget, RuleMode};
use crate::rpa::common::RpaJob;

// 筛选岗位 是否符合配置的条件
pub fn filter_verify(job: &RpaJob, config: &AppRuntimeConfig) -> bool {
    let config = config.job_filter_config.clone();

    for keyword in &config.exclude_keywords {
        if job.title.contains(keyword) {
            return false;
        }
    }

    for keyword in &config.company_exclude_keywords {
        if job.company_name.contains(keyword) {
            return false;
        }
    }

    if !config.keywords.is_empty() {
        let matched = config.keywords.iter().any(|k| job.title.contains(k));
        if !matched {
            return false;
        }
    }

    for rule in &config.regex_rules {
        let Ok(re) = regex::Regex::new(&rule.pattern) else {
            continue;
        };

        let text = match rule.target {
            MatchTarget::Title => &job.title,
            MatchTarget::Company => &job.company_name,
            MatchTarget::Description | MatchTarget::All => &job.detail,
        };

        let matches = re.is_match(text);
        match rule.mode {
            RuleMode::REJECT if matches => return false,
            RuleMode::ACCEPT if !matches && config.regex_rules.len() == 1 => return false,
            _ => {}
        }
    }

    true
}
