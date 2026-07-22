use crate::utils::{
    conditions::{
        self, get_degree_code_by_name, get_experience_code_by_name, get_job_type_code_by_name,
        get_salary_code_by_name, get_scale_code_by_name, get_stage_code_by_name,
    },
    industry, position, site,
};

pub const BOSS_JOB_SEARCH_URL: &str = "https://www.zhipin.com/web/geek/jobs";

#[derive(Debug, Default, Clone)]
pub struct JobSearchParams {
    pub city: Option<i64>,
    pub position: Option<u64>,
    pub job_type: Option<u64>,
    pub salary: Option<u64>,
    pub experience: Option<u64>,
    pub degree: Option<u64>,
    pub industry: Option<u64>,
    pub scale: Option<u64>,
    pub stage: Option<u64>,
    pub query: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct JobSearchNameParams {
    pub city: Option<String>,
    pub position: Option<String>,
    pub job_type: Option<String>,
    pub salary: Option<String>,
    pub experience: Option<String>,
    pub degree: Option<String>,
    pub industry: Option<String>,
    pub scale: Option<String>,
    pub stage: Option<String>,
    pub query: Option<String>,
}

fn append_param(pairs: &mut Vec<(String, String)>, key: &str, value: &str) {
    pairs.push((key.to_string(), value.to_string()));
}

pub fn build_job_search_url(params: &JobSearchParams) -> String {
    let mut pairs = Vec::new();

    if let Some(v) = params.city {
        append_param(&mut pairs, "city", &v.to_string());
    }
    if let Some(v) = params.position {
        append_param(&mut pairs, "position", &v.to_string());
    }
    if let Some(v) = params.job_type {
        append_param(&mut pairs, "jobType", &v.to_string());
    }
    if let Some(v) = params.salary {
        append_param(&mut pairs, "salary", &v.to_string());
    }
    if let Some(v) = params.experience {
        append_param(&mut pairs, "experience", &v.to_string());
    }
    if let Some(v) = params.degree {
        append_param(&mut pairs, "degree", &v.to_string());
    }
    if let Some(v) = params.industry {
        append_param(&mut pairs, "industry", &v.to_string());
    }
    if let Some(v) = params.scale {
        append_param(&mut pairs, "scale", &v.to_string());
    }
    if let Some(v) = params.stage {
        append_param(&mut pairs, "stage", &v.to_string());
    }
    if let Some(ref v) = params.query {
        append_param(&mut pairs, "query", v);
    }

    if pairs.is_empty() {
        BOSS_JOB_SEARCH_URL.to_string()
    } else {
        let qs = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");
        format!("{}?{}", BOSS_JOB_SEARCH_URL, qs)
    }
}

pub fn resolve_job_search_names(
    params: &JobSearchNameParams,
) -> Result<JobSearchParams, Vec<String>> {
    let mut errors = Vec::new();
    let mut resolved = JobSearchParams::default();

    if let Some(ref name) = params.city {
        match site::get_city_code_by_name(name) {
            Some(code) => resolved.city = Some(code),
            None => errors.push(format!("城市不存在: {}", name)),
        }
    }

    if let Some(ref name) = params.position {
        match position::get_code_by_name(name) {
            Some(code) => resolved.position = Some(code),
            None => errors.push(format!("职位不存在: {}", name)),
        }
    }

    if let Some(ref name) = params.job_type {
        match get_job_type_code_by_name(name) {
            Some(code) => resolved.job_type = Some(code),
            None => errors.push(format!("工作类型不存在: {}", name)),
        }
    }

    if let Some(ref name) = params.salary {
        match get_salary_code_by_name(name) {
            Some(code) => resolved.salary = Some(code),
            None => errors.push(format!("薪资范围不存在: {}", name)),
        }
    }

    if let Some(ref name) = params.experience {
        match get_experience_code_by_name(name) {
            Some(code) => resolved.experience = Some(code),
            None => errors.push(format!("经验要求不存在: {}", name)),
        }
    }

    if let Some(ref name) = params.degree {
        match get_degree_code_by_name(name) {
            Some(code) => resolved.degree = Some(code),
            None => errors.push(format!("学历要求不存在: {}", name)),
        }
    }

    if let Some(ref name) = params.industry {
        match industry::get_code_by_name(name) {
            Some(code) => resolved.industry = Some(code),
            None => errors.push(format!("行业不存在: {}", name)),
        }
    }

    if let Some(ref name) = params.scale {
        match get_scale_code_by_name(name) {
            Some(code) => resolved.scale = Some(code),
            None => errors.push(format!("公司规模不存在: {}", name)),
        }
    }

    if let Some(ref name) = params.stage {
        match get_stage_code_by_name(name) {
            Some(code) => resolved.stage = Some(code),
            None => errors.push(format!("融资阶段不存在: {}", name)),
        }
    }

    resolved.query = params.query.clone();

    if errors.is_empty() {
        Ok(resolved)
    } else {
        Err(errors)
    }
}

pub fn build_job_search_url_from_names(
    params: &JobSearchNameParams,
) -> Result<String, Vec<String>> {
    resolve_job_search_names(params).map(|resolved| build_job_search_url(&resolved))
}

pub fn validate_job_search_params(params: &JobSearchParams) -> Vec<String> {
    let mut errors = Vec::new();

    if let Some(code) = params.city {
        if site::get_name_by_code(code).is_none() {
            errors.push(format!("city code 无效: {}", code));
        }
    }

    if let Some(code) = params.position {
        if position::get_name_by_code(code).is_none() {
            errors.push(format!("position code 无效: {}", code));
        }
    }

    if let Some(code) = params.job_type {
        if get_job_type_code_by_name(
            &conditions::get_job_type_name_by_code(code).unwrap_or_default(),
        ) != Some(code)
        {
            errors.push(format!("jobType code 无效: {}", code));
        }
    }

    if let Some(code) = params.salary {
        if get_salary_code_by_name(&conditions::get_salary_name_by_code(code).unwrap_or_default())
            != Some(code)
        {
            errors.push(format!("salary code 无效: {}", code));
        }
    }

    if let Some(code) = params.experience {
        if get_experience_code_by_name(
            &conditions::get_experience_name_by_code(code).unwrap_or_default(),
        ) != Some(code)
        {
            errors.push(format!("experience code 无效: {}", code));
        }
    }

    if let Some(code) = params.degree {
        if get_degree_code_by_name(&conditions::get_degree_name_by_code(code).unwrap_or_default())
            != Some(code)
        {
            errors.push(format!("degree code 无效: {}", code));
        }
    }

    if let Some(code) = params.industry {
        if industry::get_name_by_code(code).is_none() {
            errors.push(format!("industry code 无效: {}", code));
        }
    }

    if let Some(code) = params.scale {
        if get_scale_code_by_name(&conditions::get_scale_name_by_code(code).unwrap_or_default())
            != Some(code)
        {
            errors.push(format!("scale code 无效: {}", code));
        }
    }

    if let Some(code) = params.stage {
        if get_stage_code_by_name(&conditions::get_stage_name_by_code(code).unwrap_or_default())
            != Some(code)
        {
            errors.push(format!("stage code 无效: {}", code));
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_url_with_all_params() {
        let params = JobSearchParams {
            city: Some(101010100),
            position: Some(100109),
            job_type: Some(1901),
            salary: Some(404),
            experience: Some(102),
            degree: Some(209),
            industry: Some(100020),
            scale: Some(303),
            stage: Some(801),
            query: Some("Python".to_string()),
        };

        let url = build_job_search_url(&params);
        assert!(url.contains("city=101010100"));
        assert!(url.contains("position=100109"));
        assert!(url.contains("jobType=1901"));
        assert!(url.contains("salary=404"));
        assert!(url.contains("experience=102"));
        assert!(url.contains("degree=209"));
        assert!(url.contains("industry=100020"));
        assert!(url.contains("scale=303"));
        assert!(url.contains("stage=801"));
        assert!(url.contains("query=Python"));
    }

    #[test]
    fn builds_url_from_names() {
        let params = JobSearchNameParams {
            city: Some("北京".to_string()),
            position: Some("Python".to_string()),
            job_type: Some("全职".to_string()),
            salary: Some("5-10K".to_string()),
            experience: Some("应届生".to_string()),
            degree: Some("初中及以下".to_string()),
            industry: Some("互联网".to_string()),
            scale: Some("100-499人".to_string()),
            stage: Some("未融资".to_string()),
            query: Some("Python".to_string()),
        };

        let url = build_job_search_url_from_names(&params).expect("should resolve all names");
        assert!(url.contains("city=101010100"));
        assert!(url.contains("position=100109"));
        assert!(url.contains("jobType=1901"));
        assert!(url.contains("salary=404"));
        assert!(url.contains("experience=102"));
        assert!(url.contains("degree=209"));
        assert!(url.contains("industry=100020"));
        assert!(url.contains("scale=303"));
        assert!(url.contains("stage=801"));
    }

    #[test]
    fn builds_empty_url() {
        assert_eq!(
            build_job_search_url(&JobSearchParams::default()),
            BOSS_JOB_SEARCH_URL
        );
    }

    #[test]
    fn resolves_invalid_names() {
        let params = JobSearchNameParams {
            city: Some("不存在的城市".to_string()),
            ..Default::default()
        };
        let err = build_job_search_url_from_names(&params).unwrap_err();
        assert!(err.iter().any(|e| e.contains("城市不存在")));
    }
}
