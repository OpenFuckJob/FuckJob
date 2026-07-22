use crate::rpa::common::{ChatMessage as CommonChatMessage, RpaJob};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// Boss 账户信息
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BossAccountInfo {
    // 展示的姓名
    show_name: String,
    // 性别
    gender: i64,
    // 昵称
    name: String,
    // 头像
    avatar: String,
    // HR职位
    title: String,
    // comId
    com_id: i64,
    // 加密的comId
    encrypt_com_id: String,
    is_648_vip: bool,
}

impl BossAccountInfo {
    pub fn from_base_info(base_info: &Value, is_648_vip: bool) -> Result<Self, anyhow::Error> {
        fn required_str(v: &Value, key: &str) -> Result<String, anyhow::Error> {
            v.get(key)
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("missing baseInfo.{key}"))
        }

        fn required_i64(v: &Value, key: &str) -> Result<i64, anyhow::Error> {
            v.get(key)
                .and_then(|x| x.as_i64())
                .ok_or_else(|| anyhow::anyhow!("missing baseInfo.{key}"))
        }

        Ok(BossAccountInfo {
            show_name: required_str(base_info, "showName")?,
            gender: required_i64(base_info, "gender")?,
            name: required_str(base_info, "name")?,
            avatar: required_str(base_info, "avatar")?,
            title: required_str(base_info, "title")?,
            com_id: required_i64(base_info, "comId")?,
            encrypt_com_id: required_str(base_info, "encryptComId")?,
            is_648_vip,
        })
    }

    pub fn show_name(&self) -> &str {
        &self.show_name
    }

    pub fn gender(&self) -> i64 {
        self.gender
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn avatar(&self) -> &str {
        &self.avatar
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn com_id(&self) -> i64 {
        self.com_id
    }

    pub fn encrypt_com_id(&self) -> &str {
        &self.encrypt_com_id
    }

    pub fn is_648_vip(&self) -> bool {
        self.is_648_vip
    }
}

// 岗位信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionSimpleInfo {
    pub job_name: String,
    pub salary: String,
    pub tags: Vec<String>,
    pub company_name: String,
    pub company_location: String,
    pub job_detail_url: String,
    pub company_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionDetail {
    pub keywords: Vec<String>,
    pub job_description: String,
    pub recruiter_name: String,
    pub recruiter_title: String,
    pub recruiter_active_time: String,
    pub recruiter_company: String,
}

// 未读的聊天消息
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreadChat {
    /// 列表中的索引（从 0 开始）
    pub idx: usize,
    /// 招聘者姓名，如 "宋昊"
    pub name: String,
    /// 公司名称，如 "科锐国际"
    pub company: String,
    /// 职位/头衔，如 "猎头顾问"
    pub title: String,
    /// 未读消息数量
    pub unread_count: u32,
    /// 最近消息时间，如 "19:11" 或 "04月27日"
    pub time: String,
    /// 最近一条消息内容
    pub last_message: String,
    /// 头像图片 URL
    pub avatar_url: String,
}

pub type ChatMessage = CommonChatMessage;

pub type GreetJob = RpaJob;
