use crate::config::{AppRuntimeConfig, Job51FilterConfig};
use anyhow::{bail, Result};
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Node {
    code: String,
    label: String,
    #[serde(default)]
    children: Vec<Node>,
}
#[derive(Deserialize)]
struct Catalog {
    salary: Vec<Node>,
    functions: Vec<Node>,
    company_size: Vec<Node>,
    cities: Vec<Node>,
}
fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../src/assets/resource/job51.json"))
            .expect("bundled 51job catalog must be valid JSON")
    })
}
fn contains_leaf(nodes: &[Node], code: &str) -> bool {
    nodes.iter().any(|node| {
        if node.children.is_empty() {
            node.code == code
        } else {
            contains_leaf(&node.children, code)
        }
    })
}
pub fn validate(filters: &Job51FilterConfig) -> Result<()> {
    let values = catalog();
    if filters.functions.len() > 5 {
        bail!("51job 工作职能最多选择5项");
    }
    for (label, selected, allowed) in [
        ("月薪范围", &filters.salary, &values.salary),
        ("工作职能", &filters.functions, &values.functions),
        ("公司规模", &filters.company_size, &values.company_size),
    ] {
        if selected.iter().any(|code| !contains_leaf(allowed, code)) {
            bail!("51job {label}包含无效选项，请重新选择");
        }
    }
    Ok(())
}

pub fn search_url(config: &AppRuntimeConfig, query: &str) -> Result<String> {
    let filters = &config.platform_filter_config.job51;
    validate(filters)?;
    let mut url = tauri::Url::parse("https://we.51job.com/pc/search")?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("keyword", query)
            .append_pair("searchType", "2");
        if let Some(city) = config
            .job_filter_config
            .city
            .filter(|code| *code != 100010000)
        {
            let name = crate::utils::site::get_name_by_code(city)
                .ok_or_else(|| anyhow::anyhow!("目标城市代码未识别"))?;
            let area = catalog()
                .cities
                .iter()
                .find(|node| node.label == name)
                .ok_or_else(|| anyhow::anyhow!("51job 字典未收录目标城市：{name}"))?;
            pairs.append_pair("jobArea", &area.code);
        }
        for (key, codes) in [
            ("salary", &filters.salary),
            ("function", &filters.functions),
            ("companySize", &filters.company_size),
        ] {
            if !codes.is_empty() {
                pairs.append_pair(key, &codes.join(","));
            }
        }
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_app_config;
    #[test]
    fn uses_51job_codes_and_city_without_boss_salary_or_position() {
        let mut config = default_app_config();
        config.job_filter_config.city = Some(101280600);
        config.job_filter_config.salary = 999;
        config.platform_filter_config.job51 = Job51FilterConfig {
            salary: vec!["201".into(), "07".into()],
            functions: vec!["0121".into()],
            company_size: vec!["02".into()],
        };
        let url = tauri::Url::parse(&search_url(&config, "Java 开发").unwrap()).unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["jobArea"], "040000");
        assert_eq!(params["salary"], "201,07");
        assert_eq!(params["function"], "0121");
        assert_eq!(params["companySize"], "02");
        assert_eq!(params["keyword"], "Java 开发");
    }
    #[test]
    fn rejects_invalid_codes_and_more_than_five_functions() {
        let mut filters = Job51FilterConfig::default();
        filters.salary = vec!["01".into()];
        assert!(validate(&filters).is_err());
        filters.salary.clear();
        filters.functions = vec!["0121".into(); 6];
        assert!(validate(&filters).is_err());
    }
}
