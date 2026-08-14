//! 大模型相关的类型与对外适配。
//!
//! 真正的调用逻辑已经统一到 [`crate::agent`]：这里只保留领域类型，
//! 以及给既有调用点用的薄适配层。历史上每个用途都在这里各写一遍
//! 「装参数 → 渲染 → 调用 → 解析」，同一类错误因此反复出现
//! （最典型的是变量按条件注入导致渲染必然失败，却表现为「模型没生成内容」）。

use crate::agent;
use crate::agent::tasks::{GreetTask, JobMatchTask};
use crate::config::AppRuntimeConfig;
use crate::rpa::common::RpaJob;
use serde::{Deserialize, Serialize};

pub mod service;
pub mod template;
pub mod types;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmGenerateResult {
    pub success: bool,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct JobSemanticMatch {
    pub matched: bool,
    pub score: u8,
    pub reason: String,
}

/// 在关键词和正则规则通过后，用大模型复核岗位是否符合用户的投递意图。
pub async fn evaluate_job_match(
    config: &AppRuntimeConfig,
    job: &RpaJob,
) -> Result<JobSemanticMatch, anyhow::Error> {
    let outcome = agent::run(&JobMatchTask::new(config, job), config).await?;
    Ok(outcome.output)
}

/// 生成岗位打招呼文本。
///
/// 提示词没配时返回空内容而不是报错：打招呼序列里可以只有固定文案，
/// 模型这一条缺席不该让整个岗位打招呼失败。
pub async fn generate_greet_text(
    config: AppRuntimeConfig,
    job: &RpaJob,
) -> Result<LlmGenerateResult, anyhow::Error> {
    if !config.greet_config.prompt_ready() {
        crate::logger::warning("打招呼提示词未配置，默认生成空内容")?;
        return Ok(LlmGenerateResult {
            success: true,
            data: String::new(),
        });
    }

    let outcome = agent::run(&GreetTask::new(&config, job), &config).await?;
    Ok(LlmGenerateResult {
        success: true,
        data: outcome.output,
    })
}
