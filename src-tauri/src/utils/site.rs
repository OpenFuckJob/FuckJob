use serde::Deserialize;
use std::sync::LazyLock;

use super::common::{CityMatch, CodeName};

const SITE_JSON: &str = include_str!("../resource/site.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SiteModel {
    code: i64,
    name: String,
    #[serde(default)]
    sub_level_model_list: Option<Vec<SiteModel>>,
}

static SITE_DATA: LazyLock<Vec<SiteModel>> =
    LazyLock::new(|| serde_json::from_str(SITE_JSON).expect("Failed to parse site.json"));

pub fn get_code_by_name(name: &str) -> i64 {
    get_province_code_by_name(name)
        .or_else(|| get_city_code_by_name(name))
        .unwrap_or(0)
}

pub fn get_province_code_by_name(name: &str) -> Option<i64> {
    SITE_DATA
        .iter()
        .find(|model| model.name == name)
        .map(|model| model.code)
}

pub fn get_city_code_by_name(name: &str) -> Option<i64> {
    all_city_entries()
        .into_iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.code)
}

pub fn get_name_by_code(code: i64) -> Option<String> {
    if let Some(province) = SITE_DATA.iter().find(|model| model.code == code) {
        return Some(province.name.clone());
    }

    all_city_entries()
        .into_iter()
        .find(|entry| entry.code == code)
        .map(|entry| entry.name)
}

pub fn get_city_path_by_code(code: i64) -> Option<String> {
    all_city_entries()
        .into_iter()
        .find(|entry| entry.code == code)
        .map(|entry| entry.path)
}

pub fn get_cities_by_province(province_name: &str) -> Vec<String> {
    SITE_DATA
        .iter()
        .find(|model| model.name == province_name)
        .map(|model| {
            model
                .sub_level_model_list
                .iter()
                .flatten()
                .map(|c| c.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

pub fn get_all_cities() -> Vec<String> {
    all_city_entries()
        .into_iter()
        .map(|entry| entry.path)
        .collect()
}

pub fn list_all_provinces() -> Vec<CodeName<i64>> {
    SITE_DATA
        .iter()
        .map(|model| CodeName {
            code: model.code,
            name: model.name.clone(),
        })
        .collect()
}

pub fn list_all_city_entries() -> Vec<CityMatch> {
    all_city_entries()
}

pub fn search_cities(keyword: &str) -> Vec<CityMatch> {
    all_city_entries()
        .into_iter()
        .filter(|entry| entry.name.contains(keyword) || entry.path.contains(keyword))
        .collect()
}

fn all_city_entries() -> Vec<CityMatch> {
    SITE_DATA
        .iter()
        .flat_map(|province| {
            province
                .sub_level_model_list
                .iter()
                .flatten()
                .map(move |city| CityMatch {
                    code: city.code,
                    name: city.name.clone(),
                    path: format!("{} > {}", province.name, city.name),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_code_by_province_name() {
        assert_eq!(get_code_by_name("黑龙江"), 101050000);
        assert_eq!(get_code_by_name("北京"), 101010000);
        assert_eq!(get_code_by_name("香港"), 101320000);
    }

    #[test]
    fn get_code_by_city_name() {
        assert_eq!(get_city_code_by_name("哈尔滨"), Some(101050100));
        assert_eq!(get_city_code_by_name("齐齐哈尔"), Some(101050200));
        assert_eq!(get_city_code_by_name("北京"), Some(101010100));
    }

    #[test]
    fn get_code_not_found() {
        assert_eq!(get_code_by_name("不存在的地区"), 0);
    }

    #[test]
    fn get_cities_for_province() {
        let cities = get_cities_by_province("黑龙江");
        assert!(cities.contains(&"哈尔滨".to_string()));
        assert!(cities.contains(&"齐齐哈尔".to_string()));
        assert!(cities.contains(&"牡丹江".to_string()));
        assert_eq!(cities.len(), 13);
    }

    #[test]
    fn get_cities_for_municipality() {
        let cities = get_cities_by_province("北京");
        assert_eq!(cities, vec!["北京"]);
    }

    #[test]
    fn get_all_cities_basic() {
        let all = get_all_cities();
        assert!(all.contains(&"黑龙江 > 哈尔滨".to_string()));
        assert!(all.contains(&"黑龙江 > 齐齐哈尔".to_string()));
        assert!(all.contains(&"北京 > 北京".to_string()));
        assert!(all.contains(&"香港 > 香港".to_string()));
        assert!(!all.contains(&"黑龙江".to_string()));
    }

    #[test]
    fn get_all_cities_returns_many() {
        let all = get_all_cities();
        assert!(all.len() > 300);
    }

    #[test]
    fn get_name_and_path_by_code() {
        assert_eq!(get_name_by_code(101050100), Some("哈尔滨".to_string()));
        assert_eq!(
            get_city_path_by_code(101050100),
            Some("黑龙江 > 哈尔滨".to_string())
        );
    }

    #[test]
    fn search_city_by_keyword() {
        let matches = search_cities("哈尔");
        assert!(matches.iter().any(|item| item.name == "哈尔滨"));
    }
}
