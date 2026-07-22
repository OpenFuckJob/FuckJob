use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

use crate::{
    error::AppError,
    storage::{atomic::atomic_write, migration::resolve_browser_profile, read_lock},
};

const CONFIG_FILE_NAME: &str = "app_config.yaml";
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

const DEFAULT_CONFIG_YAML: &str = include_str!("resource/app_config.yaml");

pub fn config_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    let config_dir = app_handle.path().app_config_dir().map_err(|error| {
        AppError::configuration("无法定位应用配置目录").with_detail(error.to_string())
    })?;
    Ok(config_dir.join(CONFIG_FILE_NAME))
}

fn default_greet_config() -> GreetConfig {
    GreetConfig {
        reply_prompt: None,
        default_template: Vec::new(),
    }
}

pub fn default_app_config() -> AppRuntimeConfig {
    AppRuntimeConfig {
        schema_version: CURRENT_SCHEMA_VERSION,
        onboarding_completed: false,
        llm_config: None,
        job_filter_config: JobFilterConfig {
            query: Some("Rust 工程师".to_string()),
            city: None,
            job_type: 0,
            salary: 0,
            experience: Vec::new(),
            dgree: Vec::new(),
            industry: Vec::new(),
            scale: Vec::new(),
            stage: Vec::new(),
            keywords: Vec::new(),
            exclude_keywords: Vec::new(),
            company_keywords: Vec::new(),
            company_exclude_keywords: Vec::new(),
            regex_rules: Vec::new(),
        },
        platform_filter_config: PlatformFilterConfig::default(),
        greet_config: default_greet_config(),
        replay_config: ReplayConfig {
            enable_auto_replay: false,
            templates: Vec::new(),
            enable_llm: false,
            reply_prompt: None,
            background_context: None,
        },
        browser_config: BrowserConfig {
            user_data_dir: "".to_string(),
            chrome_exe_path: None,
        },
        resume_config: ResumeConfig {
            inject_llm_context: false,
            resume_path: None,
            resume_content: None,
        },
    }
}

fn default_browser_user_data_dir(app_handle: &tauri::AppHandle) -> Result<String, AppError> {
    let app_data_dir = app_handle.path().app_data_dir().map_err(|error| {
        AppError::storage("无法定位应用数据目录").with_detail(error.to_string())
    })?;
    let path = resolve_browser_profile("", &app_data_dir);
    fs::create_dir_all(&path).map_err(|error| {
        AppError::storage("无法创建浏览器数据目录")
            .with_detail(format!("{}: {error}", path.display()))
    })?;
    Ok(path.to_string_lossy().to_string())
}

fn is_invalid_user_data_dir(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed == "null" || trimmed == "None"
}

fn ensure_browser_user_data_dir(
    app_handle: &tauri::AppHandle,
    config: &mut AppRuntimeConfig,
) -> Result<(), AppError> {
    if is_invalid_user_data_dir(&config.browser_config.user_data_dir) {
        config.browser_config.user_data_dir = default_browser_user_data_dir(app_handle)?;
    }
    Ok(())
}

fn ensure_browser_exe_path(config: &mut AppRuntimeConfig) {
    let has_explicit_path = config
        .browser_config
        .chrome_exe_path
        .as_ref()
        .is_some_and(|p| !p.trim().is_empty() && p.trim() != "null" && p.trim() != "None");

    if !has_explicit_path {
        if let Some((_name, path)) = crate::browser::detect_browser_path() {
            config.browser_config.chrome_exe_path = Some(path.to_string_lossy().to_string());
        }
    }
}

fn load_default_config_from_yaml() -> AppRuntimeConfig {
    parse_config_content(DEFAULT_CONFIG_YAML).unwrap_or_else(|_| default_app_config())
}

pub fn load_app_config(
    app_handle: tauri::AppHandle,
) -> crate::command::base::CommandResult<AppRuntimeConfig> {
    match load_app_config_inner(app_handle) {
        Ok(cfg) => crate::command::base::CommandResult::ok(cfg),
        Err(err) => crate::command::base::CommandResult::err(err),
    }
}

pub fn load_app_config_inner(app_handle: tauri::AppHandle) -> Result<AppRuntimeConfig, AppError> {
    let path = config_path(&app_handle)?;
    if !path.exists() {
        let mut config = load_default_config_from_yaml();
        ensure_browser_user_data_dir(&app_handle, &mut config)?;
        ensure_browser_exe_path(&mut config);
        save_app_config_inner(app_handle.clone(), config.clone())?;
        return Ok(config);
    }

    let mut config = read_config_file(&path)?;
    let needs_save = is_invalid_user_data_dir(&config.browser_config.user_data_dir)
        || config
            .browser_config
            .chrome_exe_path
            .as_ref()
            .is_none_or(|p| p.trim().is_empty() || p.trim() == "null" || p.trim() == "None");

    ensure_browser_user_data_dir(&app_handle, &mut config)?;
    ensure_browser_exe_path(&mut config);

    if needs_save {
        save_app_config_inner(app_handle, config.clone())?;
    }

    Ok(config)
}

pub fn save_app_config_inner(
    app_handle: tauri::AppHandle,
    config: AppRuntimeConfig,
) -> Result<(), AppError> {
    let _permit = read_lock();
    save_app_config_unlocked(app_handle, config)
}

pub(crate) fn save_app_config_unlocked(
    app_handle: tauri::AppHandle,
    mut config: AppRuntimeConfig,
) -> Result<(), AppError> {
    let path = config_path(&app_handle)?;
    validate_and_normalize(&mut config).map_err(AppError::validation)?;
    config.schema_version = CURRENT_SCHEMA_VERSION;
    let content = serde_yaml::to_string(&config).map_err(|error| {
        AppError::configuration("无法序列化应用配置").with_detail(error.to_string())
    })?;
    atomic_write(&path, content.as_bytes())
}

fn read_config_file(path: &Path) -> Result<AppRuntimeConfig, AppError> {
    let content = fs::read_to_string(path).map_err(|error| {
        AppError::storage("无法读取应用配置").with_detail(format!("{}: {error}", path.display()))
    })?;
    parse_config_content(&content).map_err(|error| {
        AppError::configuration("应用配置格式无效")
            .with_detail(format!("{}: {error}", path.display()))
    })
}

pub(crate) fn parse_config_content(content: &str) -> Result<AppRuntimeConfig, String> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|error| error.to_string())?;
    let mut config = default_app_config();
    config.schema_version = value
        .get("schema_version")
        .map(|version| serde_yaml::from_value(version.clone()))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(0);
    config.onboarding_completed = value
        .get("onboarding_completed")
        .map(|completed| serde_yaml::from_value(completed.clone()))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(false);

    if let Some(llm_config) = value.get("llm_config") {
        config.llm_config = parse_llm_config(llm_config, config.schema_version == 0)?;
    }
    if let Some(job_filter_config) = value.get("job_filter_config") {
        config.job_filter_config =
            serde_yaml::from_value(job_filter_config.clone()).map_err(|error| error.to_string())?;
    }
    if let Some(platform_filter_config) = value.get("platform_filter_config") {
        config.platform_filter_config = serde_yaml::from_value(platform_filter_config.clone())
            .map_err(|error| error.to_string())?;
    }
    if let Some(greet_config) = value.get("greet_config") {
        config.greet_config =
            serde_yaml::from_value(greet_config.clone()).map_err(|error| error.to_string())?;
    }
    if let Some(replay_config) = value.get("replay_config") {
        config.replay_config =
            serde_yaml::from_value(replay_config.clone()).map_err(|error| error.to_string())?;
    }
    if let Some(browser_config) = value.get("browser_config") {
        config.browser_config =
            serde_yaml::from_value(browser_config.clone()).map_err(|error| error.to_string())?;
    }
    if let Some(resume_config) = value.get("resume_config") {
        config.resume_config =
            serde_yaml::from_value(resume_config.clone()).map_err(|error| error.to_string())?;
    }

    validate_and_normalize(&mut config)?;
    Ok(config)
}

#[derive(Deserialize)]
struct RawLlmConfig {
    #[serde(default)]
    provider: Option<LlmProviderPreset>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

fn parse_llm_config(
    value: &serde_yaml::Value,
    allow_incomplete_legacy: bool,
) -> Result<Option<LlmConfig>, String> {
    if value.is_null() {
        return Ok(None);
    }

    let raw: RawLlmConfig =
        serde_yaml::from_value(value.clone()).map_err(|error| error.to_string())?;
    let base_url = raw.base_url.unwrap_or_default();
    let model = raw.model.unwrap_or_default();
    if base_url.trim().is_empty() || model.trim().is_empty() {
        return if allow_incomplete_legacy {
            Ok(None)
        } else {
            Err("大模型地址和模型名称不能为空".to_string())
        };
    }

    let provider = match raw.provider {
        Some(provider) => provider,
        None if allow_incomplete_legacy => infer_legacy_provider(&base_url),
        None => return Err("大模型服务预设不能为空".to_string()),
    };
    let mut config = AppRuntimeConfig {
        llm_config: Some(LlmConfig {
            provider,
            base_url,
            model,
        }),
        ..default_app_config()
    };
    validate_and_normalize(&mut config)?;
    Ok(config.llm_config)
}

/// Known legacy URLs map to their matching preset. Unknown OpenAI-compatible
/// endpoints remain usable through the custom preset.
fn infer_legacy_provider(base_url: &str) -> LlmProviderPreset {
    let normalized = base_url.to_ascii_lowercase();
    if normalized.contains("11434") || normalized.contains("ollama") {
        LlmProviderPreset::Ollama
    } else if normalized.contains("1234") || normalized.contains("lmstudio") {
        LlmProviderPreset::LmStudio
    } else if normalized.contains("deepseek") {
        LlmProviderPreset::DeepSeek
    } else if normalized.contains("dashscope") {
        LlmProviderPreset::Dashscope
    } else {
        LlmProviderPreset::Custom
    }
}

pub fn validate_and_normalize(config: &mut AppRuntimeConfig) -> Result<(), String> {
    if config.schema_version > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "应用配置版本 {} 高于当前支持的版本 {}",
            config.schema_version, CURRENT_SCHEMA_VERSION
        ));
    }

    let Some(llm_config) = config.llm_config.as_mut() else {
        return Ok(());
    };

    llm_config.base_url = llm_config.base_url.trim().trim_end_matches('/').to_string();
    llm_config.model = llm_config.model.trim().to_string();
    if llm_config.base_url.is_empty() {
        return Err("大模型地址不能为空".to_string());
    }
    if llm_config.model.is_empty() {
        return Err("模型名称不能为空".to_string());
    }
    Ok(())
}

pub fn import_app_config_inner(
    app_handle: tauri::AppHandle,
    path: &str,
) -> Result<AppRuntimeConfig, AppError> {
    let mut config = read_config_file(Path::new(path))?;
    ensure_browser_user_data_dir(&app_handle, &mut config)?;
    save_app_config_inner(app_handle, config.clone())?;
    Ok(config)
}

pub fn export_app_config_inner(path: &str, mut config: AppRuntimeConfig) -> Result<(), AppError> {
    let _permit = read_lock();
    validate_and_normalize(&mut config).map_err(AppError::validation)?;
    config.schema_version = CURRENT_SCHEMA_VERSION;
    let content = serde_yaml::to_string(&config).map_err(|error| {
        AppError::configuration("无法序列化应用配置").with_detail(error.to_string())
    })?;
    atomic_write(Path::new(path), content.as_bytes())
}

pub fn parse_resume_pdf_inner(path: &str) -> Result<String, String> {
    let path = Path::new(path);
    let is_pdf = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));

    if !is_pdf {
        return Err("请选择 PDF 格式的简历文件".to_string());
    }

    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let content =
        kreuzberg::pdf::text::extract_text_from_pdf(&bytes).map_err(|error| error.to_string())?;

    Ok(content.trim().to_string())
}

// ================================
// RPA 全局运行时配置
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AppRuntimeConfig {
    #[serde(default)]
    pub schema_version: u32,

    #[serde(default)]
    pub onboarding_completed: bool,

    #[serde(default)]
    pub llm_config: Option<LlmConfig>,

    /// 岗位筛选配置
    pub job_filter_config: JobFilterConfig,

    /// 平台专属搜索筛选配置
    #[serde(default)]
    pub platform_filter_config: PlatformFilterConfig,

    /// 主动打招呼配置
    pub greet_config: GreetConfig,

    /// 自动回复配置
    pub replay_config: ReplayConfig,

    /// 浏览器运行配置
    pub browser_config: BrowserConfig,

    /// 简历配置
    pub resume_config: ResumeConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmProviderPreset {
    Ollama,
    LmStudio,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "deepseek")]
    DeepSeek,
    Dashscope,
    Custom,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LlmConfig {
    pub provider: LlmProviderPreset,
    pub base_url: String,
    pub model: String,
}

// ================================
// 平台专属筛选配置
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct PlatformFilterConfig {
    #[serde(default)]
    pub liepin: LiepinFilterConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct LiepinFilterConfig {
    #[serde(default)]
    pub dq: Option<String>,
    #[serde(default)]
    pub salary_code: Option<String>,
    #[serde(default)]
    pub pub_time: Option<String>,
    #[serde(default)]
    pub work_year_code: Option<String>,
    #[serde(default)]
    pub comp_tag: Vec<String>,
}

// ================================
// 岗位筛选配置
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct JobFilterConfig {
    // 基础配置
    pub query: Option<String>,
    /// 目标城市
    pub city: Option<i64>,

    /// 求职类型 jobType
    pub job_type: i64,

    /// 薪资待遇
    pub salary: i64,

    /// 工作经验
    pub experience: Vec<i64>,

    /// 学历
    pub dgree: Vec<i64>,

    /// 公司行业
    pub industry: Vec<i64>,

    /// 公司规模
    pub scale: Vec<i64>,

    /// 融资情况
    pub stage: Vec<i64>,

    // 高级配置
    /// 岗位title普通关键词
    pub keywords: Vec<String>,

    /// 排除岗位title关键词
    pub exclude_keywords: Vec<String>,

    /// 公司关键字
    pub company_keywords: Vec<String>,

    /// 排除公司关键字
    pub company_exclude_keywords: Vec<String>,

    /// 正则筛选规则
    pub regex_rules: Vec<RegexRule>,
}

// ================================
// 正则规则
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RegexRule {
    /// 规则名称
    pub name: String,

    /// 正则表达式
    pub pattern: String,

    /// 匹配目标字段
    pub target: MatchTarget,

    /// 规则模式
    pub mode: RuleMode,
}

// ================================
// 匹配目标字段
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum MatchTarget {
    /// 岗位标题
    Title,

    /// 公司名称
    Company,

    /// 岗位描述
    Description,

    /// 所有字段
    All,
}

// ================================
// 规则模式
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum RuleMode {
    /// 命中则接受
    ACCEPT,
    /// 命中后直接拒绝
    REJECT,
}

// ================================
// 岗位信息
// ================================
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JobInfo {
    pub title: String,
    pub company: String,
    pub description: String,
    pub salary: Option<String>,
    pub location: String,
    pub experience_years: Option<u8>,
}

// ================================
// 匹配结果
// ================================
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MatchResult {
    /// 是否通过规则筛选
    pub matched: bool,

    /// 命中的规则名称
    pub hit_rules: Vec<String>,

    /// 拒绝原因
    pub reject_reason: Option<String>,
}

// ================================
// 主动沟通配置 优先级：LLM > Regex > default
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GreetConfig {
    /// 沟通生成提示词
    pub reply_prompt: Option<String>,

    // 默认模板
    pub default_template: Vec<ReplyResource>,
}

// ================================
// 主动回复配置 优先级：LLM > Regex
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ReplayConfig {
    /// 是否启用自动回复
    pub enable_auto_replay: bool,

    /// 正则匹配回复模板
    pub templates: Vec<ReplyTemplate>,

    /// 是否启用自动回复
    pub enable_llm: bool,

    /// 回复提示词
    pub reply_prompt: Option<String>,

    /// 背景补充
    #[serde(default)]
    pub background_context: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ReplyTemplate {
    /// 正则规则
    pub regex_rule: ReplyRegexRule,
    /// 回复内容
    pub content: Vec<ReplyResource>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ReplyRegexRule {
    /// 规则名称
    pub name: String,

    /// 正则表达式
    pub pattern: String,

    /// 匹配目标 最近的limit条聊天记录
    pub limit: i32,
}

// 回复资源
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ReplyResource {
    /// 回复类型
    pub resource_type: ReplayResourceType,
    /// 回复内容 图片 则传 图片路径
    pub content: String,
}

// 回复类型
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ReplayResourceType {
    /// 文本
    Text,
    /// 图片
    Image,
    /// 大模型生成的文本
    LLM,
}

// ================================
// 浏览器配置
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BrowserConfig {
    /// 用户数据目录
    #[serde(default)]
    pub user_data_dir: String,
    /// 浏览器执行路径
    #[serde(default)]
    pub chrome_exe_path: Option<String>,
}

// ================================
// 简历配置
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ResumeConfig {
    /// 是否注入到 LLM 上下文
    #[serde(default)]
    pub inject_llm_context: bool,

    /// 简历本地存储路径
    pub resume_path: Option<String>,

    /// 简历内容
    pub resume_content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_llm() -> LlmConfig {
        LlmConfig {
            provider: LlmProviderPreset::Ollama,
            base_url: "  http://127.0.0.1:11434/v1///  ".to_string(),
            model: "  qwen3  ".to_string(),
        }
    }

    #[test]
    fn default_config_is_version_one_and_ai_unconfigured() {
        let config = default_app_config();

        assert_eq!(config.schema_version, 1);
        assert!(!config.onboarding_completed);
        assert!(config.llm_config.is_none());
    }

    #[test]
    fn valid_llm_config_is_trimmed_and_trailing_slashes_are_removed() {
        let mut config = default_app_config();
        config.llm_config = Some(configured_llm());

        validate_and_normalize(&mut config).unwrap();

        let llm = config.llm_config.unwrap();
        assert_eq!(llm.base_url, "http://127.0.0.1:11434/v1");
        assert_eq!(llm.model, "qwen3");
    }

    #[test]
    fn invalid_non_null_llm_config_is_rejected() {
        for (base_url, model) in [
            ("", "qwen3"),
            ("   ", "qwen3"),
            ("http://localhost/v1", ""),
            ("http://localhost/v1", "   "),
        ] {
            let mut config = default_app_config();
            config.llm_config = Some(LlmConfig {
                provider: LlmProviderPreset::Custom,
                base_url: base_url.to_string(),
                model: model.to_string(),
            });

            assert!(validate_and_normalize(&mut config).is_err());
            assert!(config.llm_config.is_some());
        }
    }

    #[test]
    fn future_schema_versions_are_rejected_instead_of_downgraded() {
        let error = parse_config_content("schema_version: 2\nllm_config: null\n").unwrap_err();

        assert!(error.contains("版本"));
    }

    #[test]
    fn provider_presets_have_stable_serialized_names() {
        let cases = [
            (LlmProviderPreset::Ollama, "ollama"),
            (LlmProviderPreset::LmStudio, "lm_studio"),
            (LlmProviderPreset::OpenAi, "openai"),
            (LlmProviderPreset::DeepSeek, "deepseek"),
            (LlmProviderPreset::Dashscope, "dashscope"),
            (LlmProviderPreset::Custom, "custom"),
        ];

        for (preset, expected) in cases {
            assert_eq!(serde_yaml::to_string(&preset).unwrap().trim(), expected);
        }
    }

    #[test]
    fn incomplete_legacy_llm_config_becomes_none() {
        let config = parse_config_content(
            r#"
llm_config:
  use_custom: true
  base_url: ""
  model: qwen3
  api_key: plaintext
"#,
        )
        .unwrap();

        assert_eq!(config.schema_version, 0);
        assert!(config.llm_config.is_none());
    }

    #[test]
    fn complete_legacy_llm_config_is_preserved_and_provider_is_inferred() {
        let config = parse_config_content(
            r#"
llm_config:
  use_custom: false
  base_url: https://api.deepseek.com/
  model: deepseek-chat
  api_key: plaintext
"#,
        )
        .unwrap();

        let llm = config.llm_config.unwrap();
        assert_eq!(llm.provider, LlmProviderPreset::DeepSeek);
        assert_eq!(llm.base_url, "https://api.deepseek.com");
        assert_eq!(llm.model, "deepseek-chat");
    }

    #[test]
    fn complete_custom_legacy_llm_config_maps_to_custom_provider() {
        let config = parse_config_content(
            r#"
llm_config:
  use_custom: true
  base_url: https://llm.example.test/v1
  model: private-model
"#,
        )
        .unwrap();

        assert_eq!(
            config.llm_config.unwrap().provider,
            LlmProviderPreset::Custom
        );
    }

    #[test]
    fn legacy_advanced_llm_fields_are_ignored() {
        let config = parse_config_content(
            r#"
schema_version: 1
llm_config:
  provider: custom
  base_url: https://llm.example.test/v1
  model: private-model
  timeout_seconds: 30
  temperature: 0.7
  max_tokens: 2048
"#,
        )
        .unwrap();

        let llm = config.llm_config.unwrap();
        assert_eq!(llm.provider, LlmProviderPreset::Custom);
        assert_eq!(llm.base_url, "https://llm.example.test/v1");
        assert_eq!(llm.model, "private-model");
        let serialized = serde_yaml::to_string(&llm).unwrap();
        assert!(!serialized.contains("timeout_seconds"));
        assert!(!serialized.contains("temperature"));
        assert!(!serialized.contains("max_tokens"));
    }

    #[test]
    fn default_config_includes_empty_resume_config() {
        let config = default_app_config();

        assert!(!config.resume_config.inject_llm_context);
        assert!(config.resume_config.resume_path.is_none());
        assert!(config.resume_config.resume_content.is_none());
    }

    #[test]
    fn default_config_includes_empty_liepin_platform_filter() {
        let config = default_app_config();

        assert!(config.platform_filter_config.liepin.dq.is_none());
        assert!(config.platform_filter_config.liepin.salary_code.is_none());
        assert!(config.platform_filter_config.liepin.pub_time.is_none());
        assert!(config
            .platform_filter_config
            .liepin
            .work_year_code
            .is_none());
        assert!(config.platform_filter_config.liepin.comp_tag.is_empty());
    }

    #[test]
    fn legacy_yaml_without_resume_config_uses_default_resume_config() {
        let content = r#"
job_filter_config:
  query: Rust 工程师
  city: null
  job_type: 0
  salary: 0
  experience: []
  dgree: []
  industry: []
  scale: []
  stage: []
  keywords: []
  exclude_keywords: []
  company_keywords: []
  company_exclude_keywords: []
  regex_rules: []
llm_config:
  model: ""
  base_url: ""
  api_key: null
greet_config:
  enable_llm: false
  reply_prompt: null
  enable_regex: false
  templates: []
  default_template: []
replay_config:
  enable_auto_replay: false
  templates:
    - regex_rule:
        name: "回复示例"
        pattern: "简历|面试"
        limit: 3
      content:
        - resource_type: Text
          content: "您好，我这边方便进一步沟通。"
  enable_llm: false
  reply_prompt: null
browser_config:
  user_data_dir: ""
  chrome_exe_path: null
"#;

        let value: serde_yaml::Value = serde_yaml::from_str(content).unwrap();
        let mut config = default_app_config();
        if let Some(job_filter_config) = value.get("job_filter_config") {
            config.job_filter_config = serde_yaml::from_value(job_filter_config.clone()).unwrap();
        }
        if let Some(greet_config) = value.get("greet_config") {
            config.greet_config = serde_yaml::from_value(greet_config.clone()).unwrap();
        }
        if let Some(replay_config) = value.get("replay_config") {
            config.replay_config = serde_yaml::from_value(replay_config.clone()).unwrap();
        }
        if let Some(browser_config) = value.get("browser_config") {
            config.browser_config = serde_yaml::from_value(browser_config.clone()).unwrap();
        }

        assert!(!config.resume_config.inject_llm_context);
        assert_eq!(config.replay_config.templates[0].regex_rule.limit, 3);
        assert!(config.resume_config.resume_path.is_none());
        assert!(config.resume_config.resume_content.is_none());
    }

    #[test]
    fn parse_resume_pdf_rejects_non_pdf_file() {
        let result = parse_resume_pdf_inner("/tmp/resume.txt");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "请选择 PDF 格式的简历文件");
    }
}
