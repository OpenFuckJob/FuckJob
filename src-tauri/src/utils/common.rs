use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct CodeName<T> {
    pub code: T,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct CityMatch {
    pub code: i64,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct IndustryMatch {
    pub code: u64,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PositionMatch {
    pub code: u64,
    pub name: String,
    pub path: String,
}
