use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

use crate::{
    error::AppError,
    storage::{atomic::atomic_write, migration::resolve_browser_profile, read_lock},
};

const CONFIG_FILE_NAME: &str = "app_config.yaml";
pub const CURRENT_SCHEMA_VERSION: u32 = 3;
pub const MIN_PARALLEL_TASKS: usize = 1;
pub const MAX_PARALLEL_TASKS: usize = 2;
pub const DEFAULT_JOB_PROFILE_ID: &str = "default";
pub const DEFAULT_JOB_PROFILE_NAME: &str = "默认求职方案";

const DEFAULT_CONFIG_YAML: &str = include_str!("resource/app_config.yaml");

pub fn config_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    let config_dir = app_handle.path().app_config_dir().map_err(|error| {
        AppError::configuration("无法定位应用配置目录").with_detail(error.to_string())
    })?;
    Ok(config_dir.join(CONFIG_FILE_NAME))
}

fn default_greet_config() -> GreetConfig {
    GreetConfig {
        enable_llm: false,
        reply_prompt: None,
        default_template: Vec::new(),
    }
}

pub fn default_app_config() -> AppRuntimeConfig {
    let job_filter_config = JobFilterConfig {
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
        enable_semantic_filter: false,
        semantic_filter_intent: None,
        regex_rules: Vec::new(),
    };
    let platform_filter_config = PlatformFilterConfig::default();
    let greet_config = default_greet_config();
    let replay_config = ReplayConfig {
        enable_template_reply: false,
        templates: Vec::new(),
        enable_llm: false,
        reply_prompt: None,
        background_context: None,
        enable_auto_send_resume: default_enable_auto_send_resume(),
        max_auto_replies: default_max_auto_replies(),
        max_reply_chars: default_max_reply_chars(),
        dry_run: false,
    };
    let resume_config = ResumeConfig {
        inject_llm_context: false,
        resume_path: None,
        resume_content: None,
    };
    let default_profile = JobProfile {
        id: DEFAULT_JOB_PROFILE_ID.to_string(),
        name: DEFAULT_JOB_PROFILE_NAME.to_string(),
        description: None,
        archived: false,
        job_filter_config: job_filter_config.clone(),
        platform_filter_config: platform_filter_config.clone(),
        resume_config: resume_config.clone(),
        greet_config: greet_config.clone(),
        replay_config: replay_config.clone(),
    };

    AppRuntimeConfig {
        schema_version: CURRENT_SCHEMA_VERSION,
        onboarding_completed: false,
        llm_config: None,
        llm_fallbacks: Vec::new(),
        llm_retry_config: LlmRetryConfig::default(),
        job_profiles: vec![default_profile],
        default_job_profile_id: DEFAULT_JOB_PROFILE_ID.to_string(),
        active_job_profile: None,
        job_filter_config,
        platform_filter_config,
        greet_config,
        replay_config,
        browser_config: BrowserConfig {
            user_data_dir: "".to_string(),
            chrome_exe_path: None,
            max_parallel_tasks: default_max_parallel_tasks(),
        },
        resume_config,
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
    if let Some(llm_fallbacks) = value.get("llm_fallbacks") {
        config.llm_fallbacks =
            serde_yaml::from_value(llm_fallbacks.clone()).map_err(|error| error.to_string())?;
    }
    if let Some(llm_retry_config) = value.get("llm_retry_config") {
        config.llm_retry_config =
            serde_yaml::from_value(llm_retry_config.clone()).map_err(|error| error.to_string())?;
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
        // 兼容旧配置：早期版本没有 enable_llm 键，只要配过提示词就视为已启用 LLM 打招呼，
        // 避免升级后功能被静默关闭。用户显式写了 enable_llm 时以用户设置为准。
        if greet_config.get("enable_llm").is_none()
            && config
                .greet_config
                .reply_prompt
                .as_deref()
                .is_some_and(|prompt| !prompt.trim().is_empty())
        {
            config.greet_config.enable_llm = true;
        }

        if config.schema_version < 2 {
            migrate_greet_send_sequence(&mut config.greet_config);
        }
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
    if let Some(job_profiles) = value.get("job_profiles") {
        config.job_profiles =
            serde_yaml::from_value(job_profiles.clone()).map_err(|error| error.to_string())?;
    } else {
        // v0-v2 的五块求职配置就是唯一的求职方案。迁移时完整复制，避免升级丢失
        // 平台筛选、简历或话术；之后仍保留顶层字段作为旧调用方的执行镜像。
        config.job_profiles = vec![JobProfile::from_runtime_mirror(
            DEFAULT_JOB_PROFILE_ID,
            DEFAULT_JOB_PROFILE_NAME,
            &config,
        )];
    }
    config.default_job_profile_id = value
        .get("default_job_profile_id")
        .map(|profile_id| serde_yaml::from_value(profile_id.clone()))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| DEFAULT_JOB_PROFILE_ID.to_string());

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

/// Known legacy URLs map to their matching preset. Unknown legacy endpoints
/// fall back to OpenAI-compatible routing through the OpenAI preset.
fn infer_legacy_provider(base_url: &str) -> LlmProviderPreset {
    let normalized = base_url.to_ascii_lowercase();
    if normalized.contains("anthropic") {
        LlmProviderPreset::Anthropic
    } else if normalized.contains("deepseek") {
        LlmProviderPreset::DeepSeek
    } else if normalized.contains("minimax") {
        LlmProviderPreset::MiniMax
    } else if normalized.contains("moonshot") {
        LlmProviderPreset::Moonshot
    } else if normalized.contains("11434") || normalized.contains("ollama") {
        LlmProviderPreset::Ollama
    } else if normalized.contains("openrouter") {
        LlmProviderPreset::OpenRouter
    } else if normalized.contains("xiaomimimo") {
        LlmProviderPreset::XiaomiMimo
    } else if normalized.contains("z.ai") {
        LlmProviderPreset::ZAi
    } else {
        LlmProviderPreset::OpenAi
    }
}

pub fn validate_and_normalize(config: &mut AppRuntimeConfig) -> Result<(), String> {
    if config.schema_version > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "应用配置版本 {} 高于当前支持的版本 {}",
            config.schema_version, CURRENT_SCHEMA_VERSION
        ));
    }

    normalize_llm_retry_config(&mut config.llm_retry_config);
    normalize_llm_fallbacks(&mut config.llm_fallbacks)?;
    normalize_job_profiles(config)?;
    config.browser_config.max_parallel_tasks = config
        .browser_config
        .max_parallel_tasks
        .clamp(MIN_PARALLEL_TASKS, MAX_PARALLEL_TASKS);

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

/// v1 在模板缺少 LLM 条目时会在运行期把生成内容隐式插到第一条。
/// v2 改为完全显式的发送序列，因此升级时只需补出这个条目；运行期不再保留兼容分支。
fn migrate_greet_send_sequence(greet: &mut GreetConfig) {
    let mut found_llm = false;
    greet.default_template.retain(|resource| {
        if resource.resource_type != ReplayResourceType::LLM {
            return true;
        }
        if found_llm {
            return false;
        }
        found_llm = true;
        true
    });

    let prompt_ready = greet
        .reply_prompt
        .as_deref()
        .is_some_and(|prompt| !prompt.trim().is_empty());
    if greet.enable_llm && prompt_ready && !found_llm {
        greet.default_template.insert(
            0,
            GreetResource::new(ReplayResourceType::LLM, String::new()),
        );
    }
}

fn validate_greet_template(greet: &GreetConfig) -> Result<(), String> {
    let llm_count = greet
        .default_template
        .iter()
        .filter(|resource| resource.resource_type == ReplayResourceType::LLM)
        .count();
    if llm_count > 1 {
        return Err("打招呼发送序列最多只能包含一条 LLM 内容".to_string());
    }
    Ok(())
}

/// 重试参数超出合理区间时直接夹紧，而不是拒绝保存：
/// 这类数值填错不影响功能正确性，没必要把用户挡在配置页外面。
fn normalize_llm_retry_config(retry: &mut LlmRetryConfig) {
    retry.network_retry_attempts = retry.network_retry_attempts.min(MAX_NETWORK_RETRY_ATTEMPTS);
    retry.retry_base_delay_ms = retry
        .retry_base_delay_ms
        .clamp(MIN_RETRY_BASE_DELAY_MS, MAX_RETRY_BASE_DELAY_MS);
}

/// 标识只允许字母、数字、下划线和连字符：它会被拼进 keyring 条目名，
/// 放开任意字符会让密钥存取在不同平台上行为不一致。
fn is_valid_entry_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn normalize_llm_fallbacks(fallbacks: &mut Vec<LlmProviderEntry>) -> Result<(), String> {
    for entry in fallbacks.iter_mut() {
        entry.id = entry.id.trim().to_string();
        entry.base_url = entry.base_url.trim().trim_end_matches('/').to_string();
        entry.model = entry.model.trim().to_string();
        entry.label = entry
            .label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_string);
    }

    // 界面上新增一行后还没来得及填写就保存，属于常见操作，静默丢弃即可；
    // 填了一半才是真的配置错误，必须挡下来提醒用户。
    fallbacks.retain(|entry| !(entry.base_url.is_empty() && entry.model.is_empty()));

    let mut seen_ids: HashSet<&str> = HashSet::new();
    for entry in fallbacks.iter() {
        if !is_valid_entry_id(&entry.id) {
            return Err("备用大模型服务的标识无效，仅支持字母、数字、下划线和连字符".to_string());
        }
        if entry.id == PRIMARY_LLM_ENTRY_ID {
            return Err(format!(
                "备用大模型服务不能使用保留标识 {PRIMARY_LLM_ENTRY_ID}"
            ));
        }
        if !seen_ids.insert(entry.id.as_str()) {
            return Err(format!("备用大模型服务标识重复：{}", entry.id));
        }
        if entry.base_url.is_empty() {
            return Err("备用大模型服务的地址不能为空".to_string());
        }
        if entry.model.is_empty() {
            return Err("备用大模型服务的模型名称不能为空".to_string());
        }
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

    /// 主用大模型服务，同时也是降级链的首位
    #[serde(default)]
    pub llm_config: Option<LlmConfig>,

    /// 主用服务不可用时按顺序尝试的备用服务
    #[serde(default)]
    pub llm_fallbacks: Vec<LlmProviderEntry>,

    /// 大模型调用的重试策略
    #[serde(default)]
    pub llm_retry_config: LlmRetryConfig,

    /// 可复用的求职方案卡。方案卡是持久化主数据，顶层五块配置仅作为默认方案的兼容镜像。
    #[serde(default)]
    pub job_profiles: Vec<JobProfile>,

    /// 未显式选择方案时使用的方案标识
    #[serde(default = "default_job_profile_id")]
    pub default_job_profile_id: String,

    /// 队列执行快照所绑定的方案元信息，不写入配置文件。
    #[serde(skip)]
    pub active_job_profile: Option<ActiveJobProfile>,

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

fn default_job_profile_id() -> String {
    DEFAULT_JOB_PROFILE_ID.to_string()
}

/// 一张完整、可独立执行的求职方案卡。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct JobProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub archived: bool,
    pub job_filter_config: JobFilterConfig,
    #[serde(default)]
    pub platform_filter_config: PlatformFilterConfig,
    pub resume_config: ResumeConfig,
    pub greet_config: GreetConfig,
    pub replay_config: ReplayConfig,
}

impl JobProfile {
    fn from_runtime_mirror(id: &str, name: &str, config: &AppRuntimeConfig) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            archived: false,
            job_filter_config: config.job_filter_config.clone(),
            platform_filter_config: config.platform_filter_config.clone(),
            resume_config: config.resume_config.clone(),
            greet_config: config.greet_config.clone(),
            replay_config: config.replay_config.clone(),
        }
    }
}

/// 运行中的配置快照实际绑定到哪张方案卡。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveJobProfile {
    pub id: String,
    pub name: String,
    pub snapshot_id: String,
}

/// 方案解析结果：既提供旧 RPA 可直接消费的扁平配置，也提供队列持久化所需元信息。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedJobProfile {
    pub config: AppRuntimeConfig,
    pub profile_id: String,
    pub profile_name: String,
    pub snapshot_id: String,
}

/// 将选中方案解析为旧执行层所需的扁平配置快照。
/// `profile_id` 为空时解析默认方案；归档方案不能用于创建新任务。
pub fn resolve_job_profile(
    config: &AppRuntimeConfig,
    profile_id: Option<&str>,
) -> Result<ResolvedJobProfile, String> {
    let mut snapshot = config.clone();
    validate_and_normalize(&mut snapshot)?;
    let profile = snapshot.job_profile(profile_id)?.clone();
    if profile.archived {
        return Err(format!(
            "求职方案「{}」已归档，不能用于新任务",
            profile.name
        ));
    }

    let snapshot_id = job_profile_snapshot_id(&profile)?;
    let active = ActiveJobProfile {
        id: profile.id.clone(),
        name: profile.name.clone(),
        snapshot_id: snapshot_id.clone(),
    };
    snapshot.job_filter_config = profile.job_filter_config;
    snapshot.platform_filter_config = profile.platform_filter_config;
    snapshot.resume_config = profile.resume_config;
    snapshot.greet_config = profile.greet_config;
    snapshot.replay_config = profile.replay_config;
    snapshot.active_job_profile = Some(active);

    Ok(ResolvedJobProfile {
        config: snapshot,
        profile_id: profile.id,
        profile_name: profile.name,
        snapshot_id,
    })
}

fn job_profile_snapshot_id(profile: &JobProfile) -> Result<String, String> {
    // 对稳定方案身份和实际执行内容做指纹。重命名或说明调整不会产生新版本；
    // 但两张不同方案即使内容暂时相同也必须保持快照身份隔离，否则后保存的
    // 方案元数据会覆盖前一张卡的历史归属。
    let bytes = serde_json::to_vec(&(
        &profile.id,
        &profile.job_filter_config,
        &profile.platform_filter_config,
        &profile.resume_config,
        &profile.greet_config,
        &profile.replay_config,
    ))
    .map_err(|error| error.to_string())?;
    // FNV-1a 是跨进程、跨平台确定的内容指纹；这里只用于识别相同执行内容，不承担安全用途。
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("jp-{hash:016x}"))
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmProviderPreset {
    Anthropic,
    #[serde(rename = "deepseek")]
    DeepSeek,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "openai_responses")]
    OpenAiResponses,
    #[serde(rename = "minimax")]
    MiniMax,
    Moonshot,
    Ollama,
    #[serde(rename = "openrouter")]
    OpenRouter,
    XiaomiMimo,
    #[serde(rename = "zai")]
    ZAi,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LlmConfig {
    pub provider: LlmProviderPreset,
    pub base_url: String,
    pub model: String,
}

/// 主用服务在降级链中的保留标识。它的 API Key 仍存放在旧的 keyring 条目里，
/// 这样老用户升级后无需重新填写密钥。
pub const PRIMARY_LLM_ENTRY_ID: &str = "primary";

/// 降级链中的一个备用大模型服务。
/// 主用服务仍由 `llm_config` 承载，本列表按顺序作为它的后备。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LlmProviderEntry {
    /// 稳定标识，用于关联独立存储的 API Key；由前端生成，重排序时不得变化
    pub id: String,

    /// 展示名称，为空时界面回退到「服务预设 + 模型名」
    #[serde(default)]
    pub label: Option<String>,

    pub provider: LlmProviderPreset,

    pub base_url: String,

    pub model: String,

    /// 是否参与降级链。关闭后保留配置但不再被调用
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// 大模型调用的重试策略
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LlmRetryConfig {
    /// 网络类瞬时故障的额外重试次数（不含首次请求），0 表示不重试
    #[serde(default = "default_network_retry_attempts")]
    pub network_retry_attempts: u32,

    /// 首次重试前的等待毫秒数，之后按指数退避
    #[serde(default = "default_retry_base_delay_ms")]
    pub retry_base_delay_ms: u64,
}

fn default_network_retry_attempts() -> u32 {
    2
}

fn default_retry_base_delay_ms() -> u64 {
    500
}

impl Default for LlmRetryConfig {
    fn default() -> Self {
        Self {
            network_retry_attempts: default_network_retry_attempts(),
            retry_base_delay_ms: default_retry_base_delay_ms(),
        }
    }
}

/// 重试次数上限。再多也救不回真正故障的服务，只会拖慢整轮求职
pub const MAX_NETWORK_RETRY_ATTEMPTS: u32 = 5;
/// 重试等待时长的允许区间（毫秒）
pub const MIN_RETRY_BASE_DELAY_MS: u64 = 100;
pub const MAX_RETRY_BASE_DELAY_MS: u64 = 10_000;

/// 降级链中的一环，屏蔽「主用配置」与「备用条目」之间的结构差异。
/// 调用方只需按顺序遍历，不必关心某一环来自哪张表。
#[derive(Debug, Clone, PartialEq)]
pub struct LlmChainLink {
    pub id: String,
    pub label: Option<String>,
    pub provider: LlmProviderPreset,
    pub base_url: String,
    pub model: String,
}

impl LlmChainLink {
    /// 日志与错误提示里使用的可读名称
    pub fn display_name(&self) -> String {
        match self.label.as_deref() {
            Some(label) if !label.trim().is_empty() => label.to_string(),
            _ => self.model.clone(),
        }
    }

    /// 是否为主用服务（其 API Key 存放在旧的 keyring 条目中）
    pub fn is_primary(&self) -> bool {
        self.id == PRIMARY_LLM_ENTRY_ID
    }
}

impl AppRuntimeConfig {
    /// 按稳定标识查找方案；未传标识时使用默认方案。
    pub fn job_profile(&self, profile_id: Option<&str>) -> Result<&JobProfile, String> {
        let profile_id = profile_id
            .map(str::trim)
            .filter(|profile_id| !profile_id.is_empty())
            .unwrap_or(self.default_job_profile_id.as_str());
        self.job_profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| format!("求职方案不存在：{profile_id}"))
    }

    /// 按调用顺序返回大模型降级链：主用服务在前，其后是处于启用状态的备用服务。
    /// 未配置主用服务时返回空链，调用方据此报「请先配置大模型服务」。
    pub fn llm_chain(&self) -> Vec<LlmChainLink> {
        // 全局的 AI 功能门禁都以「是否配置了主用服务」为准，
        // 这里必须保持一致：没有主用服务时整条链不可用，而不是退而使用备用服务。
        let Some(primary) = self.llm_config.as_ref() else {
            return Vec::new();
        };

        let mut chain = Vec::with_capacity(self.llm_fallbacks.len() + 1);
        chain.push(LlmChainLink {
            id: PRIMARY_LLM_ENTRY_ID.to_string(),
            label: None,
            provider: primary.provider.clone(),
            base_url: primary.base_url.clone(),
            model: primary.model.clone(),
        });

        chain.extend(
            self.llm_fallbacks
                .iter()
                .filter(|entry| entry.enabled)
                .map(|entry| LlmChainLink {
                    id: entry.id.clone(),
                    label: entry.label.clone(),
                    provider: entry.provider.clone(),
                    base_url: entry.base_url.clone(),
                    model: entry.model.clone(),
                }),
        );

        chain
    }
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

    /// 是否在确定性规则通过后使用大模型复核岗位意图
    #[serde(default)]
    pub enable_semantic_filter: bool,

    /// 用户期望投递的岗位画像（自然语言）
    #[serde(default)]
    pub semantic_filter_intent: Option<String>,

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
    /// 是否启用大模型生成打招呼内容
    #[serde(default)]
    pub enable_llm: bool,

    /// 沟通生成提示词
    pub reply_prompt: Option<String>,

    // 默认模板
    pub default_template: Vec<GreetResource>,
}

impl GreetConfig {
    pub fn prompt_ready(&self) -> bool {
        self.reply_prompt
            .as_deref()
            .is_some_and(|prompt| !prompt.trim().is_empty())
    }

    pub fn has_enabled_llm_resource(&self) -> bool {
        self.default_template
            .iter()
            .any(|resource| resource.enabled && resource.resource_type == ReplayResourceType::LLM)
    }

    pub fn llm_resource_ready(&self) -> bool {
        self.enable_llm && self.prompt_ready() && self.has_enabled_llm_resource()
    }

    pub fn has_sendable_resource(&self) -> bool {
        self.default_template.iter().any(|resource| {
            resource.enabled
                && match resource.resource_type {
                    ReplayResourceType::LLM => self.llm_resource_ready(),
                    _ => !resource.content.trim().is_empty(),
                }
        })
    }
}

// ================================
// 主动回复配置
//
// 两个开关各代表一条独立的回复路径，不是「总开关 + 子选项」的关系：
// `enable_llm` 是模型决策链路，`enable_template_reply` 是正则模板链路。
// 同时开着时模板命中即短路——用户显式写死的话术比模型现编的更该被信任。
// 界面上这两个 bool 合并呈现为一个四选一的回复策略，用户不必自己推演组合。
// ================================
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ReplayConfig {
    /// 是否启用正则模板回复。只管模板这一条路径，不是自动回复的总开关。
    ///
    /// 旧名 `enable_auto_replay` 读着像总开关，代码里也真被当成总开关用过一段时间，
    /// 于是「只开 LLM」的方案会被整条跳过。别名保留是为了读得进旧配置文件
    #[serde(alias = "enable_auto_replay")]
    pub enable_template_reply: bool,

    /// 正则匹配回复模板
    pub templates: Vec<ReplyTemplate>,

    /// 是否启用大模型生成回复内容
    pub enable_llm: bool,

    /// 回复提示词
    pub reply_prompt: Option<String>,

    /// 背景补充
    #[serde(default)]
    pub background_context: Option<String>,

    /// 是否允许模型自主决定投递简历。
    /// 关掉之后模型仍会判断时机，但只回消息，投递交回人工
    #[serde(default = "default_enable_auto_send_resume")]
    pub enable_auto_send_resume: bool,

    /// 单个会话累计自动回复条数上限，达到后转人工。
    /// 没有上限时模型会和 HR 无限客套下去
    #[serde(default = "default_max_auto_replies")]
    pub max_auto_replies: usize,

    /// 单条自动回复的字数上限。超长的求职消息本身就不像真人写的
    #[serde(default = "default_max_reply_chars")]
    pub max_reply_chars: usize,

    /// 演练模式：判断与生成照常，但不实际发送。
    /// 首次启用自动回复时建议先开着跑一轮，确认生成质量再关掉
    #[serde(default)]
    pub dry_run: bool,
}

impl ReplayConfig {
    /// 本方案是否要处理未读会话。
    ///
    /// 任意一条回复路径开着就有事可做。此前这里只看模板开关，
    /// 于是「只开 LLM 回复」的方案会在读完会话之后被整条链路跳过
    pub fn auto_reply_enabled(&self) -> bool {
        self.enable_llm || self.enable_template_reply
    }

    pub fn prompt_ready(&self) -> bool {
        self.reply_prompt
            .as_deref()
            .is_some_and(|prompt| !prompt.trim().is_empty())
    }

    /// 模板链路是否有内容可发。全是空话术的模板等于没配
    pub fn has_sendable_template(&self) -> bool {
        self.templates.iter().any(|template| {
            template
                .content
                .iter()
                .any(|resource| !resource.content.trim().is_empty())
        })
    }
}

fn default_enable_auto_send_resume() -> bool {
    true
}

fn default_max_auto_replies() -> usize {
    5
}

fn default_max_reply_chars() -> usize {
    200
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

// 打招呼发送资源
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GreetResource {
    /// 是否参与发送。旧配置缺少此字段时默认启用；启用状态不落盘，保持配置简洁。
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// 回复类型
    pub resource_type: ReplayResourceType,
    /// 回复内容 图片 则传 图片路径
    pub content: String,
}

impl GreetResource {
    pub fn new(resource_type: ReplayResourceType, content: String) -> Self {
        Self {
            enabled: true,
            resource_type,
            content,
        }
    }
}

fn is_true(value: &bool) -> bool {
    *value
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
    /// 同时执行的自动化任务上限。当前仅开放跨平台双任务并行。
    #[serde(default = "default_max_parallel_tasks")]
    pub max_parallel_tasks: usize,
}

fn normalize_job_profiles(config: &mut AppRuntimeConfig) -> Result<(), String> {
    // 备份导入等旧路径可能直接把 v0-v2 YAML 反序列化为 AppRuntimeConfig，绕过
    // parse_config_content。这里补上同等迁移；v3 显式保存空列表仍按无效配置拒绝。
    if config.job_profiles.is_empty() && config.schema_version < 3 {
        config.job_profiles = vec![JobProfile::from_runtime_mirror(
            DEFAULT_JOB_PROFILE_ID,
            DEFAULT_JOB_PROFILE_NAME,
            config,
        )];
        config.default_job_profile_id = DEFAULT_JOB_PROFILE_ID.to_string();
    }

    let mut ids = HashSet::new();
    let mut active_count = 0usize;

    for profile in &mut config.job_profiles {
        profile.id = profile.id.trim().to_string();
        profile.name = profile.name.trim().to_string();
        profile.description = profile
            .description
            .take()
            .map(|description| description.trim().to_string())
            .filter(|description| !description.is_empty());

        if profile.id.is_empty() {
            return Err("求职方案标识不能为空".to_string());
        }
        if profile.name.is_empty() {
            return Err(format!("求职方案 {} 的名称不能为空", profile.id));
        }
        if !ids.insert(profile.id.clone()) {
            return Err(format!("求职方案标识重复：{}", profile.id));
        }
        if !profile.archived {
            active_count += 1;
        }
        validate_greet_template(&profile.greet_config)
            .map_err(|error| format!("求职方案「{}」无效：{error}", profile.name))?;
    }

    if active_count == 0 {
        return Err("至少需要保留一张未归档的求职方案".to_string());
    }

    config.default_job_profile_id = config.default_job_profile_id.trim().to_string();
    let default_profile = config
        .job_profiles
        .iter()
        .find(|profile| profile.id == config.default_job_profile_id)
        .ok_or_else(|| "默认求职方案不存在".to_string())?;
    if default_profile.archived {
        return Err("默认求职方案不能是已归档方案".to_string());
    }

    config.job_filter_config = default_profile.job_filter_config.clone();
    config.platform_filter_config = default_profile.platform_filter_config.clone();
    config.resume_config = default_profile.resume_config.clone();
    config.greet_config = default_profile.greet_config.clone();
    config.replay_config = default_profile.replay_config.clone();
    Ok(())
}

fn default_max_parallel_tasks() -> usize {
    2
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
    fn default_config_uses_current_version_and_ai_is_unconfigured() {
        let config = default_app_config();

        assert_eq!(config.schema_version, CURRENT_SCHEMA_VERSION);
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
                provider: LlmProviderPreset::OpenAi,
                base_url: base_url.to_string(),
                model: model.to_string(),
            });

            assert!(validate_and_normalize(&mut config).is_err());
            assert!(config.llm_config.is_some());
        }
    }

    #[test]
    fn future_schema_versions_are_rejected_instead_of_downgraded() {
        let error = parse_config_content(&format!(
            "schema_version: {}\nllm_config: null\n",
            CURRENT_SCHEMA_VERSION + 1
        ))
        .unwrap_err();

        assert!(error.contains("版本"));
    }

    #[test]
    fn provider_presets_have_stable_serialized_names() {
        let cases = [
            (LlmProviderPreset::Anthropic, "anthropic"),
            (LlmProviderPreset::DeepSeek, "deepseek"),
            (LlmProviderPreset::OpenAi, "openai"),
            (LlmProviderPreset::OpenAiResponses, "openai_responses"),
            (LlmProviderPreset::MiniMax, "minimax"),
            (LlmProviderPreset::Moonshot, "moonshot"),
            (LlmProviderPreset::Ollama, "ollama"),
            (LlmProviderPreset::OpenRouter, "openrouter"),
            (LlmProviderPreset::XiaomiMimo, "xiaomi_mimo"),
            (LlmProviderPreset::ZAi, "zai"),
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
    fn complete_unknown_legacy_llm_config_maps_to_openai_provider() {
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
            LlmProviderPreset::OpenAi
        );
    }

    #[test]
    fn legacy_advanced_llm_fields_are_ignored() {
        let config = parse_config_content(
            r#"
schema_version: 1
llm_config:
  provider: openai
  base_url: https://llm.example.test/v1
  model: private-model
  timeout_seconds: 30
  temperature: 0.7
  max_tokens: 2048
"#,
        )
        .unwrap();

        let llm = config.llm_config.unwrap();
        assert_eq!(llm.provider, LlmProviderPreset::OpenAi);
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
    fn legacy_greet_config_without_enable_llm_but_with_prompt_is_migrated_to_enabled() {
        let config = parse_config_content(
            r#"
schema_version: 1
greet_config:
  reply_prompt: "请根据岗位信息生成打招呼内容"
  default_template: []
"#,
        )
        .unwrap();

        assert!(config.greet_config.enable_llm);
        assert_eq!(config.greet_config.default_template.len(), 1);
        assert_eq!(
            config.greet_config.default_template[0].resource_type,
            ReplayResourceType::LLM
        );
    }

    #[test]
    fn legacy_greet_config_without_prompt_keeps_llm_disabled() {
        for prompt in ["null", "\"\"", "\"   \""] {
            let config = parse_config_content(&format!(
                r#"
schema_version: 1
greet_config:
  reply_prompt: {prompt}
  default_template: []
"#
            ))
            .unwrap();

            assert!(
                !config.greet_config.enable_llm,
                "提示词为 {prompt} 时不应自动启用 LLM 打招呼"
            );
        }
    }

    #[test]
    fn explicit_greet_enable_llm_false_is_respected_over_migration() {
        let config = parse_config_content(
            r#"
schema_version: 1
greet_config:
  enable_llm: false
  reply_prompt: "请根据岗位信息生成打招呼内容"
  default_template: []
"#,
        )
        .unwrap();

        assert!(!config.greet_config.enable_llm);
    }

    /// 这个字段改过名。旧配置里它叫 enable_auto_replay，升级后必须照样读得进来，
    /// 否则用户什么都没动，模板回复就静默关掉了
    #[test]
    fn the_legacy_auto_replay_key_still_loads_into_the_renamed_field() {
        let config = parse_config_content(
            r#"
schema_version: 1
replay_config:
  enable_auto_replay: true
  templates: []
  enable_llm: false
  reply_prompt: null
"#,
        )
        .unwrap();

        assert!(config.replay_config.enable_template_reply);
    }

    #[test]
    fn greet_enable_llm_is_serialized_back_to_yaml() {
        let mut greet = default_greet_config();
        greet.enable_llm = true;

        let serialized = serde_yaml::to_string(&greet).unwrap();

        assert!(serialized.contains("enable_llm: true"));

        let restored: GreetConfig = serde_yaml::from_str(&serialized).unwrap();
        assert!(restored.enable_llm);
    }

    #[test]
    fn legacy_resource_without_enabled_field_defaults_to_enabled() {
        let resource: GreetResource = serde_yaml::from_str(
            r#"
resource_type: Text
content: "您好"
"#,
        )
        .unwrap();

        assert!(resource.enabled);
        let serialized = serde_yaml::to_string(&resource).unwrap();
        assert!(!serialized.contains("enabled:"));
    }

    #[test]
    fn disabled_resource_is_serialized_explicitly() {
        let resource = GreetResource {
            enabled: false,
            resource_type: ReplayResourceType::Text,
            content: "保留但不发送".to_string(),
        };

        let serialized = serde_yaml::to_string(&resource).unwrap();

        assert!(serialized.contains("enabled: false"));
    }

    #[test]
    fn v1_greet_without_llm_slot_is_migrated_once_to_an_explicit_first_item() {
        let config = parse_config_content(
            r#"
schema_version: 1
greet_config:
  enable_llm: true
  reply_prompt: "生成内容"
  default_template:
    - resource_type: Text
      content: "固定内容"
"#,
        )
        .unwrap();

        assert_eq!(config.greet_config.default_template.len(), 2);
        assert_eq!(
            config.greet_config.default_template[0].resource_type,
            ReplayResourceType::LLM
        );
        assert_eq!(config.greet_config.default_template[1].content, "固定内容");

        let serialized = serde_yaml::to_string(&config).unwrap();
        let restored = parse_config_content(&serialized).unwrap();
        assert_eq!(restored.greet_config.default_template.len(), 2);
    }

    #[test]
    fn multiple_llm_items_are_rejected() {
        let mut config = default_app_config();
        config.job_profiles[0].greet_config.default_template = vec![
            GreetResource::new(ReplayResourceType::LLM, String::new()),
            GreetResource::new(ReplayResourceType::LLM, String::new()),
        ];

        let error = validate_and_normalize(&mut config).unwrap_err();
        assert!(error.contains("最多只能包含一条 LLM"));
    }

    #[test]
    fn v2_flat_config_is_migrated_losslessly_to_default_profile() {
        let config = parse_config_content(
            r#"
schema_version: 2
job_filter_config:
  query: AI 应用工程师
  city: 101020100
  job_type: 1901
  salary: 405
  experience: [103]
  dgree: [203]
  industry: []
  scale: []
  stage: []
  keywords: [Agent]
  exclude_keywords: [外包]
  company_keywords: []
  company_exclude_keywords: []
  enable_semantic_filter: true
  semantic_filter_intent: "AI Agent 应用开发"
  regex_rules: []
platform_filter_config:
  liepin:
    dq: "020"
    salary_code: "40$60"
    pub_time: "3"
    work_year_code: "1$3"
    comp_tag: ["104"]
greet_config:
  enable_llm: false
  reply_prompt: null
  default_template:
    - resource_type: Text
      content: "您好"
replay_config:
  enable_auto_replay: false
  templates: []
  enable_llm: false
  reply_prompt: null
  background_context: "可立即到岗"
browser_config:
  user_data_dir: profile
  chrome_exe_path: null
resume_config:
  inject_llm_context: true
  resume_path: resume.pdf
  resume_content: "Agent 项目经验"
"#,
        )
        .unwrap();

        assert_eq!(config.job_profiles.len(), 1);
        assert_eq!(config.default_job_profile_id, DEFAULT_JOB_PROFILE_ID);
        let profile = &config.job_profiles[0];
        assert_eq!(profile.name, DEFAULT_JOB_PROFILE_NAME);
        assert_eq!(
            profile.job_filter_config.query.as_deref(),
            Some("AI 应用工程师")
        );
        assert_eq!(
            profile.platform_filter_config.liepin.dq.as_deref(),
            Some("020")
        );
        assert_eq!(
            profile.resume_config.resume_content.as_deref(),
            Some("Agent 项目经验")
        );
        assert_eq!(profile.greet_config.default_template[0].content, "您好");
        assert_eq!(
            profile.replay_config.background_context.as_deref(),
            Some("可立即到岗")
        );
    }

    #[test]
    fn job_profile_validation_rejects_invalid_collections_and_default() {
        let mut config = default_app_config();
        config.job_profiles[0].name = "   ".to_string();
        assert!(validate_and_normalize(&mut config)
            .unwrap_err()
            .contains("名称不能为空"));

        let mut config = default_app_config();
        config.job_profiles.push(config.job_profiles[0].clone());
        assert!(validate_and_normalize(&mut config)
            .unwrap_err()
            .contains("标识重复"));

        let mut config = default_app_config();
        config.default_job_profile_id = "missing".to_string();
        assert!(validate_and_normalize(&mut config)
            .unwrap_err()
            .contains("默认求职方案不存在"));

        let mut config = default_app_config();
        config.job_profiles[0].archived = true;
        assert!(validate_and_normalize(&mut config)
            .unwrap_err()
            .contains("至少需要保留"));

        let mut config = default_app_config();
        let mut available = config.job_profiles[0].clone();
        available.id = "available".to_string();
        config.job_profiles.push(available);
        config.job_profiles[0].archived = true;
        assert!(validate_and_normalize(&mut config)
            .unwrap_err()
            .contains("默认求职方案不能是已归档"));
    }

    #[test]
    fn v3_explicit_empty_profile_collection_is_rejected() {
        let error = parse_config_content(
            r#"
schema_version: 3
default_job_profile_id: default
job_profiles: []
"#,
        )
        .unwrap_err();

        assert!(error.contains("至少需要保留"));
    }

    #[test]
    fn direct_v2_deserialization_is_migrated_during_normalization() {
        let serialized = serde_yaml::to_string(&default_app_config()).unwrap();
        let mut value: serde_yaml::Value = serde_yaml::from_str(&serialized).unwrap();
        let mapping = value.as_mapping_mut().unwrap();
        mapping.remove(serde_yaml::Value::String("job_profiles".to_string()));
        mapping.remove(serde_yaml::Value::String(
            "default_job_profile_id".to_string(),
        ));
        mapping.insert(
            serde_yaml::Value::String("schema_version".to_string()),
            serde_yaml::Value::Number(2.into()),
        );
        let legacy = serde_yaml::to_string(&value).unwrap();
        let mut config: AppRuntimeConfig = serde_yaml::from_str(&legacy).unwrap();

        validate_and_normalize(&mut config).unwrap();

        assert_eq!(config.job_profiles.len(), 1);
        assert_eq!(config.job_profiles[0].id, DEFAULT_JOB_PROFILE_ID);
        assert_eq!(
            config.job_profiles[0].job_filter_config,
            config.job_filter_config
        );
    }

    #[test]
    fn profile_config_round_trip_preserves_cards_and_default_mirror() {
        let mut config = default_app_config();
        let mut second = config.job_profiles[0].clone();
        second.id = "rust-backend".to_string();
        second.name = "Rust 后端".to_string();
        second.description = Some("  高并发服务端方向  ".to_string());
        second.job_filter_config.query = Some("Rust 后端工程师".to_string());
        config.job_profiles.push(second);
        config.default_job_profile_id = "rust-backend".to_string();
        validate_and_normalize(&mut config).unwrap();

        let serialized = serde_yaml::to_string(&config).unwrap();
        let restored = parse_config_content(&serialized).unwrap();

        assert_eq!(restored.job_profiles.len(), 2);
        assert_eq!(restored.default_job_profile_id, "rust-backend");
        assert_eq!(
            restored.job_profiles[1].description.as_deref(),
            Some("高并发服务端方向")
        );
        assert_eq!(
            restored.job_filter_config.query.as_deref(),
            Some("Rust 后端工程师")
        );
        assert_eq!(restored.active_job_profile, None);
    }

    #[test]
    fn normalization_mirrors_default_profile_into_flat_config() {
        let mut config = default_app_config();
        config.job_profiles[0].job_filter_config.query = Some("后端工程师".to_string());
        config.job_profiles[0].resume_config.resume_content = Some("Rust 项目".to_string());
        config.job_filter_config.query = Some("过期镜像".to_string());

        validate_and_normalize(&mut config).unwrap();

        assert_eq!(
            config.job_filter_config.query.as_deref(),
            Some("后端工程师")
        );
        assert_eq!(
            config.resume_config.resume_content.as_deref(),
            Some("Rust 项目")
        );
    }

    #[test]
    fn resolve_job_profile_builds_flat_immutable_execution_snapshot() {
        let mut config = default_app_config();
        let mut ai_profile = config.job_profiles[0].clone();
        ai_profile.id = "ai-agent".to_string();
        ai_profile.name = "AI Agent".to_string();
        ai_profile.job_filter_config.query = Some("AI Agent 工程师".to_string());
        ai_profile.resume_config.resume_content = Some("定向简历".to_string());
        config.job_profiles.push(ai_profile);

        let resolved = resolve_job_profile(&config, Some("ai-agent")).unwrap();

        assert_eq!(resolved.profile_id, "ai-agent");
        assert_eq!(resolved.profile_name, "AI Agent");
        assert!(resolved.snapshot_id.starts_with("jp-"));
        assert_eq!(
            resolved.config.job_filter_config.query.as_deref(),
            Some("AI Agent 工程师")
        );
        assert_eq!(
            resolved.config.resume_config.resume_content.as_deref(),
            Some("定向简历")
        );
        assert_eq!(
            resolved.config.active_job_profile.as_ref().unwrap().id,
            "ai-agent"
        );
        assert!(!serde_yaml::to_string(&resolved.config)
            .unwrap()
            .contains("active_job_profile"));

        // 后续编辑原方案不会污染已经解析完成的队列快照。
        config.job_profiles[1].resume_config.resume_content = Some("新版简历".to_string());
        assert_eq!(
            resolved.config.resume_config.resume_content.as_deref(),
            Some("定向简历")
        );
    }

    #[test]
    fn snapshot_id_tracks_execution_content_but_not_profile_label() {
        let config = default_app_config();
        let first = resolve_job_profile(&config, None).unwrap();
        let mut renamed = config.clone();
        renamed.job_profiles[0].name = "重命名方案".to_string();
        let second = resolve_job_profile(&renamed, None).unwrap();
        assert_eq!(first.snapshot_id, second.snapshot_id);

        renamed.job_profiles[0].job_filter_config.query = Some("Java".to_string());
        let changed = resolve_job_profile(&renamed, None).unwrap();
        assert_ne!(first.snapshot_id, changed.snapshot_id);

        let mut copied = config.clone();
        copied.job_profiles[0].id = "copied-profile".to_string();
        copied.default_job_profile_id = "copied-profile".to_string();
        let copied = resolve_job_profile(&copied, None).unwrap();
        assert_ne!(first.snapshot_id, copied.snapshot_id);
    }

    fn fallback_entry(id: &str, model: &str) -> LlmProviderEntry {
        LlmProviderEntry {
            id: id.to_string(),
            label: None,
            provider: LlmProviderPreset::OpenAi,
            base_url: "https://llm.example.test/v1".to_string(),
            model: model.to_string(),
            enabled: true,
        }
    }

    #[test]
    fn legacy_config_without_llm_chain_fields_uses_defaults() {
        let config = parse_config_content("schema_version: 1\nllm_config: null\n").unwrap();

        assert!(config.llm_fallbacks.is_empty());
        assert_eq!(config.llm_retry_config, LlmRetryConfig::default());
        assert_eq!(config.llm_retry_config.network_retry_attempts, 2);
        assert_eq!(config.llm_retry_config.retry_base_delay_ms, 500);
    }

    #[test]
    fn llm_chain_puts_primary_first_and_skips_disabled_fallbacks() {
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderPreset::DeepSeek,
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
        });
        let mut disabled = fallback_entry("backup-b", "gpt-4o-mini");
        disabled.enabled = false;
        config.llm_fallbacks = vec![fallback_entry("backup-a", "qwen-max"), disabled];

        let chain = config.llm_chain();

        assert_eq!(chain.len(), 2);
        assert!(chain[0].is_primary());
        assert_eq!(chain[0].model, "deepseek-chat");
        assert_eq!(chain[1].id, "backup-a");
        assert!(!chain[1].is_primary());
    }

    #[test]
    fn llm_chain_is_empty_without_primary_service() {
        let mut config = default_app_config();
        config.llm_fallbacks = vec![fallback_entry("backup-a", "qwen-max")];

        assert!(config.llm_chain().is_empty());
    }

    #[test]
    fn chain_link_display_name_prefers_label_then_model() {
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderPreset::OpenAi,
            base_url: "https://llm.example.test/v1".to_string(),
            model: "primary-model".to_string(),
        });
        let mut labeled = fallback_entry("backup-a", "qwen-max");
        labeled.label = Some("阿里备用".to_string());
        config.llm_fallbacks = vec![labeled, fallback_entry("backup-b", "gpt-4o-mini")];

        let chain = config.llm_chain();

        assert_eq!(chain[0].display_name(), "primary-model");
        assert_eq!(chain[1].display_name(), "阿里备用");
        assert_eq!(chain[2].display_name(), "gpt-4o-mini");
    }

    #[test]
    fn retry_settings_are_clamped_instead_of_rejected() {
        let mut config = default_app_config();
        config.llm_retry_config = LlmRetryConfig {
            network_retry_attempts: 99,
            retry_base_delay_ms: 1,
        };

        validate_and_normalize(&mut config).unwrap();

        assert_eq!(
            config.llm_retry_config.network_retry_attempts,
            MAX_NETWORK_RETRY_ATTEMPTS
        );
        assert_eq!(
            config.llm_retry_config.retry_base_delay_ms,
            MIN_RETRY_BASE_DELAY_MS
        );

        config.llm_retry_config.retry_base_delay_ms = 999_999;
        validate_and_normalize(&mut config).unwrap();
        assert_eq!(
            config.llm_retry_config.retry_base_delay_ms,
            MAX_RETRY_BASE_DELAY_MS
        );
    }

    #[test]
    fn browser_parallelism_defaults_to_two_and_is_clamped_to_supported_range() {
        let legacy = parse_config_content(
            "schema_version: 2\nbrowser_config:\n  user_data_dir: profile\n  chrome_exe_path: null\n",
        )
        .unwrap();
        assert_eq!(legacy.browser_config.max_parallel_tasks, 2);

        let mut config = default_app_config();
        config.browser_config.max_parallel_tasks = 99;
        validate_and_normalize(&mut config).unwrap();
        assert_eq!(config.browser_config.max_parallel_tasks, MAX_PARALLEL_TASKS);

        config.browser_config.max_parallel_tasks = 0;
        validate_and_normalize(&mut config).unwrap();
        assert_eq!(config.browser_config.max_parallel_tasks, MIN_PARALLEL_TASKS);
    }

    #[test]
    fn blank_fallback_rows_are_dropped_but_half_filled_rows_are_rejected() {
        let mut config = default_app_config();
        let mut blank = fallback_entry("backup-blank", "");
        blank.base_url = "   ".to_string();
        config.llm_fallbacks = vec![blank, fallback_entry("backup-a", "qwen-max")];

        validate_and_normalize(&mut config).unwrap();
        assert_eq!(config.llm_fallbacks.len(), 1);
        assert_eq!(config.llm_fallbacks[0].id, "backup-a");

        config.llm_fallbacks = vec![fallback_entry("backup-a", "")];
        let error = validate_and_normalize(&mut config).unwrap_err();
        assert!(error.contains("模型名称不能为空"));
    }

    #[test]
    fn fallback_ids_must_be_unique_safe_and_not_reserved() {
        let mut config = default_app_config();

        config.llm_fallbacks = vec![
            fallback_entry("backup-a", "qwen-max"),
            fallback_entry("backup-a", "gpt-4o-mini"),
        ];
        assert!(validate_and_normalize(&mut config)
            .unwrap_err()
            .contains("标识重复"));

        config.llm_fallbacks = vec![fallback_entry(PRIMARY_LLM_ENTRY_ID, "qwen-max")];
        assert!(validate_and_normalize(&mut config)
            .unwrap_err()
            .contains("保留标识"));

        config.llm_fallbacks = vec![fallback_entry("backup a/b", "qwen-max")];
        assert!(validate_and_normalize(&mut config)
            .unwrap_err()
            .contains("标识无效"));
    }

    #[test]
    fn fallback_urls_are_trimmed_like_the_primary_service() {
        let mut config = default_app_config();
        let mut entry = fallback_entry("backup-a", "  qwen-max  ");
        entry.base_url = "  https://llm.example.test/v1///  ".to_string();
        entry.label = Some("   ".to_string());
        config.llm_fallbacks = vec![entry];

        validate_and_normalize(&mut config).unwrap();

        assert_eq!(
            config.llm_fallbacks[0].base_url,
            "https://llm.example.test/v1"
        );
        assert_eq!(config.llm_fallbacks[0].model, "qwen-max");
        assert_eq!(config.llm_fallbacks[0].label, None);
    }

    #[test]
    fn parse_resume_pdf_rejects_non_pdf_file() {
        let result = parse_resume_pdf_inner("/tmp/resume.txt");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "请选择 PDF 格式的简历文件");
    }
}
