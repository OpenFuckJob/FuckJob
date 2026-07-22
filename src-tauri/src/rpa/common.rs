use serde::{Deserialize, Serialize};

use super::run_flow::PlatformKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpaJob {
    pub platform: PlatformKind,
    pub platform_job_id: String,
    pub title: String,
    pub company_name: String,
    pub detail: String,
    pub salary: String,
    pub location: Option<String>,
    pub detail_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub mid: i64,
    pub received: bool,
    pub text: String,
    pub time: i64,
    pub from_name: String,
}
