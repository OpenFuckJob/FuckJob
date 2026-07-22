use crate::command::base::CommandResult;
use serde::{Deserialize, Serialize};

const RESUME_TEMPLATES_JSON: &str = include_str!("../resource/resumes/data.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeTemplate {
    pub id: String,
    pub year: String,
    pub job: String,
    pub title: String,
    pub tag: Vec<String>,
    pub thumbnail: String,
    pub template: String,
    pub author: String,
    pub avatar: String,
    pub theme: String,
    pub color: String,
    #[serde(default)]
    pub collect: i64,
    pub update_time: i64,
}

#[tauri::command]
pub fn get_resume_templates() -> CommandResult<Vec<ResumeTemplate>> {
    match parse_resume_templates() {
        Ok(templates) => CommandResult::ok(templates),
        Err(error) => CommandResult::err(error),
    }
}

fn parse_resume_templates() -> Result<Vec<ResumeTemplate>, String> {
    serde_json::from_str(RESUME_TEMPLATES_JSON).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_resume_templates;

    #[test]
    fn bundled_resume_templates_can_be_parsed() {
        let templates = parse_resume_templates().unwrap();

        assert!(!templates.is_empty());
        assert!(!templates[0].id.is_empty());
        assert!(templates[0].template.contains("#"));
    }
}
