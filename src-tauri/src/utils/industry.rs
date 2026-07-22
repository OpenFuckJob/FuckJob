use serde::Deserialize;
use std::sync::LazyLock;

use super::common::{CodeName, IndustryMatch};

const INDUSTRY_JSON: &str = include_str!("../resource/industry.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndustryNode {
    code: u64,
    name: String,
    #[serde(default)]
    sub_level_model_list: Option<Vec<IndustryNode>>,
}

static INDUSTRY_DATA: LazyLock<Vec<IndustryNode>> =
    LazyLock::new(|| serde_json::from_str(INDUSTRY_JSON).expect("Failed to parse industry.json"));

pub fn get_code_by_name(name: &str) -> Option<u64> {
    list_all_industries()
        .into_iter()
        .find(|item| item.name == name)
        .map(|item| item.code)
}

pub fn get_name_by_code(code: u64) -> Option<String> {
    list_all_industries()
        .into_iter()
        .find(|item| item.code == code)
        .map(|item| item.name)
}

pub fn get_path_by_code(code: u64) -> Option<String> {
    list_all_industries()
        .into_iter()
        .find(|item| item.code == code)
        .map(|item| item.path)
}

pub fn list_top_industries() -> Vec<CodeName<u64>> {
    INDUSTRY_DATA
        .iter()
        .map(|item| CodeName {
            code: item.code,
            name: item.name.clone(),
        })
        .collect()
}

pub fn list_sub_industries(parent_name: &str) -> Vec<CodeName<u64>> {
    INDUSTRY_DATA
        .iter()
        .find(|item| item.name == parent_name)
        .map(|item| {
            item.sub_level_model_list
                .iter()
                .flatten()
                .map(|child| CodeName {
                    code: child.code,
                    name: child.name.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn list_all_industries() -> Vec<IndustryMatch> {
    let mut result = Vec::new();

    for top in INDUSTRY_DATA.iter() {
        result.push(IndustryMatch {
            code: top.code,
            name: top.name.clone(),
            path: top.name.clone(),
        });

        if let Some(children) = &top.sub_level_model_list {
            for child in children {
                result.push(IndustryMatch {
                    code: child.code,
                    name: child.name.clone(),
                    path: format!("{} > {}", top.name, child.name),
                });
            }
        }
    }

    result
}

pub fn search_industries(keyword: &str) -> Vec<IndustryMatch> {
    list_all_industries()
        .into_iter()
        .filter(|item| item.name.contains(keyword) || item.path.contains(keyword))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_industry_code_by_name() {
        assert_eq!(get_code_by_name("人工智能"), Some(100028));
    }

    #[test]
    fn finds_industry_name_and_path_by_code() {
        assert_eq!(get_name_by_code(100028), Some("人工智能".to_string()));
        assert_eq!(
            get_path_by_code(100028),
            Some("互联网/AI > 人工智能".to_string())
        );
    }

    #[test]
    fn lists_top_and_sub_industries() {
        assert!(list_top_industries()
            .iter()
            .any(|item| item.name == "互联网/AI"));
        assert!(list_sub_industries("互联网/AI")
            .iter()
            .any(|item| item.name == "人工智能"));
    }

    #[test]
    fn searches_industries() {
        let matches = search_industries("智能");
        assert!(matches.iter().any(|item| item.name == "人工智能"));
    }
}
