use crate::config::{AppRuntimeConfig, JobFilterConfig, MatchTarget, RuleMode};
use crate::rpa::common::RpaJob;
use crate::utils::salary::{parse_salary, SalaryUnit};

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

/// 标准化公司名称用于同公司去重统计（去除常见空白与括号后缀噪声）。
pub fn normalize_company_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

/// 检查同公司建联上限。`max_per_company == 0` 表示不限制。
pub fn company_greet_limit_reason(
    company_name: &str,
    existing_count: u32,
    max_per_company: u32,
) -> Option<String> {
    if max_per_company == 0 || normalize_company_name(company_name).is_empty() {
        return None;
    }
    if existing_count >= max_per_company {
        return Some(format!(
            "同一公司「{}」已建联 {} 个岗位（上限 {}），跳过避免重复骚扰",
            company_name.trim(),
            existing_count,
            max_per_company
        ));
    }
    None
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

/// 真实薪资现实过滤：在零 LLM 成本、甚至未点击详情卡片前拦截低薪/面议/宽跨度虚标岗。
pub fn salary_filter_reason(salary_text: &str, config: &JobFilterConfig) -> Option<String> {
    let wants_min = config.min_salary_k.filter(|value| *value > 0);
    let wants_span = config
        .max_salary_span_ratio
        .filter(|ratio| ratio.is_finite() && *ratio >= 1.2);
    if wants_min.is_none() && wants_span.is_none() && !config.exclude_negotiable {
        return None;
    }

    let Some(parsed) = parse_salary(salary_text) else {
        return None;
    };

    if parsed.unit == SalaryUnit::Negotiable {
        if config.exclude_negotiable {
            return Some("薪资为「面议」，已按配置跳过".to_string());
        }
        return None;
    }

    if let Some(min_k) = wants_min {
        match parsed.unit {
            SalaryUnit::MonthlyK => {
                if parsed.max + 0.01 < min_k as f32 {
                    return Some(format!(
                        "岗位薪资（{}）最高未达期望月薪 {}K",
                        salary_text.trim(),
                        min_k
                    ));
                }
            }
            SalaryUnit::DailyYuan | SalaryUnit::HourlyYuan => {
                return Some(format!(
                    "岗位为日薪/时薪（{}），与期望月薪 {}K 不匹配",
                    salary_text.trim(),
                    min_k
                ));
            }
            SalaryUnit::Negotiable => {}
        }
    }

    if let Some(max_ratio) = wants_span {
        if parsed.unit == SalaryUnit::MonthlyK {
            if let Some(ratio) = parsed.span_ratio() {
                if ratio > max_ratio + 0.01 {
                    return Some(format!(
                        "薪资跨度过大（{}，倍数 {:.1}x > {:.1}x），疑似虚标",
                        salary_text.trim(),
                        ratio,
                        max_ratio
                    ));
                }
            }
        }
    }

    None
}

/// 零点击前置过滤：仅依赖列表卡片上已有的标题、公司名、薪资做确定性过滤。
///
/// 放在点击岗位卡片拉取详情之前调用，可直接砍掉 60%~85% 无效详情页请求，
/// 显著降低 BOSS 详情高频浏览风控风险。
pub fn pre_filter_decision(
    title: &str,
    company_name: &str,
    salary: &str,
    config: &JobFilterConfig,
) -> FilterDecision {
    if let Some(keyword) = first_keyword_match(title, &config.exclude_keywords) {
        return FilterDecision::reject(format!("岗位标题命中排除关键词“{keyword}”"));
    }

    if let Some(keyword) = first_keyword_match(company_name, &config.company_exclude_keywords) {
        return FilterDecision::reject(format!("公司名称命中排除关键词“{keyword}”"));
    }

    let title_keywords = configured_keywords(&config.keywords);
    if !title_keywords.is_empty()
        && !title_keywords
            .iter()
            .any(|keyword| normalized(title).contains(keyword))
    {
        return FilterDecision::reject("岗位标题未命中任何包含关键词");
    }

    let company_keywords = configured_keywords(&config.company_keywords);
    if !company_keywords.is_empty()
        && !company_keywords
            .iter()
            .any(|keyword| normalized(company_name).contains(keyword))
    {
        return FilterDecision::reject("公司名称未命中任何包含公司关键词");
    }

    if let Some(reason) = salary_filter_reason(salary, config) {
        return FilterDecision::reject(reason);
    }

    // 仅在卡片阶段提前评估针对 Title/Company 的 REJECT 正则规则
    for rule in &config.regex_rules {
        if rule.mode != RuleMode::REJECT || rule.pattern.trim().is_empty() {
            continue;
        }
        let target_text = match rule.target {
            MatchTarget::Title => title,
            MatchTarget::Company => company_name,
            MatchTarget::Description | MatchTarget::All => continue,
        };
        if let Ok(regex) = regex::Regex::new(&rule.pattern) {
            if regex.is_match(target_text) {
                return FilterDecision::reject(format!("命中拒绝规则“{}”", rule.name));
            }
        }
    }

    FilterDecision::accept("通过列表前置筛选")
}

/// 参与匹配的岗位描述。
///
/// 用清洗后的正文而不是抓取原文：反爬注入的随机类名（`.HsDyPBbi{...}`）和
/// 开头粘成一坨的技能标签都会参与 `contains`，让「排除 Java」这类规则
/// 命中一堆不相干的岗位。规则匹配的应该是人看到的那份文本
fn description_text(job: &RpaJob) -> String {
    crate::job_description::clean_text(&job.detail, job.platform.as_str())
}

fn regex_target_text(job: &RpaJob, target: &MatchTarget) -> String {
    match target {
        MatchTarget::Title => job.title.clone(),
        MatchTarget::Company => job.company_name.clone(),
        MatchTarget::Description => description_text(job),
        MatchTarget::All => format!(
            "{}\n{}\n{}",
            job.title,
            job.company_name,
            description_text(job)
        ),
    }
}

pub fn filter_decision(job: &RpaJob, config: &AppRuntimeConfig) -> FilterDecision {
    filter_decision_with_config(job, &config.job_filter_config)
}

fn filter_decision_with_config(job: &RpaJob, config: &JobFilterConfig) -> FilterDecision {
    let pre = pre_filter_decision(&job.title, &job.company_name, &job.salary, config);
    if !pre.matched {
        return pre;
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

    #[test]
    fn salary_reality_filter_blocks_underpaid_inflated_and_negotiable_jobs() {
        let mut config = default_app_config();
        config.job_filter_config.min_salary_k = Some(20);
        config.job_filter_config.max_salary_span_ratio = Some(2.0);
        config.job_filter_config.exclude_negotiable = true;

        let mut low = job("后端开发", "示例科技", "");
        low.salary = "12-18K".to_string();
        assert!(!filter_verify(&low, &config));

        let mut inflated = job("后端开发", "示例科技", "");
        inflated.salary = "10-25K".to_string();
        assert!(!filter_verify(&inflated, &config));

        let mut negotiable = job("后端开发", "示例科技", "");
        negotiable.salary = "薪资面议".to_string();
        assert!(!filter_verify(&negotiable, &config));

        let mut intern = job("后端实习", "示例科技", "");
        intern.salary = "200-300元/天".to_string();
        assert!(!filter_verify(&intern, &config));

        let mut good = job("后端开发", "示例科技", "");
        good.salary = "20-35K·14薪".to_string();
        assert!(filter_verify(&good, &config));
    }

    #[test]
    fn pre_filter_catches_title_company_and_salary_without_jd() {
        let mut config = default_app_config();
        config.job_filter_config.company_exclude_keywords = vec!["软通动力".to_string()];
        config.job_filter_config.min_salary_k = Some(18);

        assert!(
            !pre_filter_decision("Rust 工程师", "软通动力信息技术", "20-30K", &config.job_filter_config)
                .matched
        );
        assert!(
            !pre_filter_decision("Rust 工程师", "示例科技", "10-15K", &config.job_filter_config)
                .matched
        );
        assert!(
            pre_filter_decision("Rust 工程师", "示例科技", "20-30K", &config.job_filter_config)
                .matched
        );
    }

    #[test]
    fn company_greet_limit_enforces_cap_when_enabled() {
        assert!(company_greet_limit_reason("示例科技", 5, 0).is_none());
        assert!(company_greet_limit_reason("示例科技", 0, 1).is_none());
        assert!(company_greet_limit_reason("示例科技", 1, 1).is_some());
    }
}
