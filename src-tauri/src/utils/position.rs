use serde::Deserialize;
use std::sync::LazyLock;

use super::common::{CodeName, PositionMatch};

const POSITION_JSON: &str = include_str!("../resource/position.simple.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionNode {
    code: u64,
    name: String,
    #[serde(default)]
    sub_level_model_list: Option<Vec<PositionNode>>,
}

static POSITION_DATA: LazyLock<Vec<PositionNode>> = LazyLock::new(|| {
    serde_json::from_str(POSITION_JSON).expect("Failed to parse position.simple.json")
});

pub fn get_code_by_name(name: &str) -> Option<u64> {
    list_all_positions()
        .into_iter()
        .find(|item| item.name == name)
        .map(|item| item.code)
}

pub fn get_name_by_code(code: u64) -> Option<String> {
    list_all_positions()
        .into_iter()
        .find(|item| item.code == code)
        .map(|item| item.name)
}

pub fn get_path_by_code(code: u64) -> Option<String> {
    list_all_positions()
        .into_iter()
        .find(|item| item.code == code)
        .map(|item| item.path)
}

pub fn list_top_positions() -> Vec<CodeName<u64>> {
    POSITION_DATA
        .iter()
        .map(|item| CodeName {
            code: item.code,
            name: item.name.clone(),
        })
        .collect()
}

pub fn list_sub_positions(parent_name: &str) -> Vec<CodeName<u64>> {
    let mut result = Vec::new();
    for top in POSITION_DATA.iter() {
        if top.name == parent_name {
            if let Some(children) = &top.sub_level_model_list {
                for child in children {
                    result.push(CodeName {
                        code: child.code,
                        name: child.name.clone(),
                    });
                }
            }
            return result;
        }
        if let Some(children) = &top.sub_level_model_list {
            for child in children {
                if child.name == parent_name {
                    if let Some(grandchildren) = &child.sub_level_model_list {
                        for gc in grandchildren {
                            result.push(CodeName {
                                code: gc.code,
                                name: gc.name.clone(),
                            });
                        }
                    }
                    return result;
                }
            }
        }
    }
    result
}

pub fn list_all_positions() -> Vec<PositionMatch> {
    let mut result = Vec::new();

    for top in POSITION_DATA.iter() {
        if let Some(children) = &top.sub_level_model_list {
            for child in children {
                if let Some(grandchildren) = &child.sub_level_model_list {
                    for gc in grandchildren {
                        result.push(PositionMatch {
                            code: gc.code,
                            name: gc.name.clone(),
                            path: format!("{} > {} > {}", top.name, child.name, gc.name),
                        });
                    }
                } else {
                    result.push(PositionMatch {
                        code: child.code,
                        name: child.name.clone(),
                        path: format!("{} > {}", top.name, child.name),
                    });
                }
            }
        }
    }

    result
}

pub fn search_positions(keyword: &str) -> Vec<PositionMatch> {
    list_all_positions()
        .into_iter()
        .filter(|item| item.name.contains(keyword) || item.path.contains(keyword))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_position_by_code_and_name() {
        assert_eq!(get_code_by_name("Python"), Some(100109));
        assert_eq!(get_name_by_code(100109), Some("Python".to_string()));
    }

    #[test]
    fn finds_position_path_by_code() {
        let path = get_path_by_code(100109);
        assert!(path.is_some());
        assert!(path.unwrap().contains("Python"));
    }

    #[test]
    fn searches_positions() {
        let matches = search_positions("开发");
        assert!(matches.len() > 1);
    }
}
