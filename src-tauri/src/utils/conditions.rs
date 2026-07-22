use serde::Deserialize;
use std::sync::LazyLock;

use super::common::CodeName;

const CONDITIONS_JSON: &str = include_str!("../resource/conditions.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConditionsData {
    pay_type_list: Vec<CodeName<u64>>,
    experience_list: Vec<CodeName<u64>>,
    salary_list: Vec<SalaryOption>,
    stage_list: Vec<CodeName<u64>>,
    scale_list: Vec<CodeName<u64>>,
    part_time_list: Vec<CodeName<u64>>,
    degree_list: Vec<CodeName<u64>>,
    job_type_list: Vec<CodeName<u64>>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SalaryOption {
    pub code: u64,
    pub name: String,
    pub low_salary: u64,
    pub high_salary: u64,
}

static CONDITIONS_DATA: LazyLock<ConditionsData> = LazyLock::new(|| {
    serde_json::from_str(CONDITIONS_JSON).expect("Failed to parse conditions.json")
});

pub fn list_all_filter_groups() -> Vec<&'static str> {
    vec![
        "payTypeList",
        "experienceList",
        "salaryList",
        "stageList",
        "scaleList",
        "partTimeList",
        "degreeList",
        "jobTypeList",
    ]
}

pub fn list_job_types() -> Vec<CodeName<u64>> {
    CONDITIONS_DATA.job_type_list.clone()
}

pub fn list_experiences() -> Vec<CodeName<u64>> {
    CONDITIONS_DATA.experience_list.clone()
}

pub fn list_degrees() -> Vec<CodeName<u64>> {
    CONDITIONS_DATA.degree_list.clone()
}

pub fn list_scales() -> Vec<CodeName<u64>> {
    CONDITIONS_DATA.scale_list.clone()
}

pub fn list_stages() -> Vec<CodeName<u64>> {
    CONDITIONS_DATA.stage_list.clone()
}

pub fn list_pay_types() -> Vec<CodeName<u64>> {
    CONDITIONS_DATA.pay_type_list.clone()
}

pub fn list_part_times() -> Vec<CodeName<u64>> {
    CONDITIONS_DATA.part_time_list.clone()
}

pub fn list_salary_ranges() -> Vec<SalaryOption> {
    CONDITIONS_DATA.salary_list.clone()
}

pub fn list_filter_group(group_name: &str) -> Vec<CodeName<u64>> {
    match group_name {
        "payTypeList" => list_pay_types(),
        "experienceList" => list_experiences(),
        "stageList" => list_stages(),
        "scaleList" => list_scales(),
        "partTimeList" => list_part_times(),
        "degreeList" => list_degrees(),
        "jobTypeList" => list_job_types(),
        "salaryList" => CONDITIONS_DATA
            .salary_list
            .iter()
            .map(|item| CodeName {
                code: item.code,
                name: item.name.clone(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub fn search_filter_value(group_name: &str, keyword: &str) -> Vec<CodeName<u64>> {
    list_filter_group(group_name)
        .into_iter()
        .filter(|item| item.name.contains(keyword))
        .collect()
}

macro_rules! code_name_lookup {
    ($code_fn:ident, $name_fn:ident, $list_fn:ident) => {
        pub fn $code_fn(name: &str) -> Option<u64> {
            $list_fn()
                .into_iter()
                .find(|item| item.name == name)
                .map(|item| item.code)
        }

        pub fn $name_fn(code: u64) -> Option<String> {
            $list_fn()
                .into_iter()
                .find(|item| item.code == code)
                .map(|item| item.name)
        }
    };
}

code_name_lookup!(
    get_job_type_code_by_name,
    get_job_type_name_by_code,
    list_job_types
);
code_name_lookup!(
    get_experience_code_by_name,
    get_experience_name_by_code,
    list_experiences
);
code_name_lookup!(
    get_degree_code_by_name,
    get_degree_name_by_code,
    list_degrees
);
code_name_lookup!(get_scale_code_by_name, get_scale_name_by_code, list_scales);
code_name_lookup!(get_stage_code_by_name, get_stage_name_by_code, list_stages);
code_name_lookup!(
    get_pay_type_code_by_name,
    get_pay_type_name_by_code,
    list_pay_types
);
code_name_lookup!(
    get_part_time_code_by_name,
    get_part_time_name_by_code,
    list_part_times
);

pub fn get_salary_code_by_name(name: &str) -> Option<u64> {
    CONDITIONS_DATA
        .salary_list
        .iter()
        .find(|item| item.name == name)
        .map(|item| item.code)
}

pub fn get_salary_name_by_code(code: u64) -> Option<String> {
    CONDITIONS_DATA
        .salary_list
        .iter()
        .find(|item| item.code == code)
        .map(|item| item.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_salary_by_code_and_name() {
        assert_eq!(get_salary_code_by_name("5-10K"), Some(404));
        assert_eq!(get_salary_name_by_code(404), Some("5-10K".to_string()));
    }

    #[test]
    fn finds_job_type() {
        assert_eq!(get_job_type_code_by_name("全职"), Some(1901));
        assert_eq!(get_job_type_name_by_code(1901), Some("全职".to_string()));
    }

    #[test]
    fn lists_group_and_searches() {
        assert!(list_filter_group("degreeList")
            .iter()
            .any(|item| item.name == "本科"));
        assert!(search_filter_value("salaryList", "5-10")
            .iter()
            .any(|item| item.code == 404));
    }
}
