use crate::config::{AppRuntimeConfig, LlmChainLink, LlmProviderPreset, LlmRetryConfig};
use crate::credential::ResolvedCredential;
use crate::error::{AppError, AppErrorCode};
use crate::llm::types::{ConnectionReport, LlmResponse};
use futures::StreamExt;
use rig::client::CompletionClient;
use rig::completion::{CompletionError, CompletionModel};
use rig::streaming::StreamedAssistantContent;
use serde_json::Value;
use std::future::Future;
use std::time::Duration;
use tokio::time::timeout;

#[derive(Clone, Debug)]
pub struct LlmService {
    backend: LlmBackend,
    model: String,
    provider: LlmProviderPreset,
    /// 单个服务内部的重试策略，来自 `AppRuntimeConfig::llm_retry_config`
    retry: LlmRetryConfig,
    /// 日志里展示的服务名称，用于让用户分辨重试/降级发生在哪一环
    display_name: String,
}

#[derive(Clone, Debug)]
enum LlmBackend {
    Anthropic(rig::providers::anthropic::Client),
    DeepSeek(rig::providers::deepseek::Client),
    OpenAiCompatible(rig::providers::openai::CompletionsClient),
    OpenAiResponses(rig::providers::openai::Client),
    MiniMax(rig::providers::minimax::Client),
    Moonshot(rig::providers::moonshot::Client),
    Ollama(rig::providers::ollama::Client),
    OpenRouter(rig::providers::openrouter::Client),
    XiaomiMimo(rig::providers::xiaomimimo::Client),
    ZAi(rig::providers::zai::Client),
}

impl LlmService {
    /// 兼容既有调用点：等价于「取降级链首位（即主用服务）构建单个服务」。
    /// 需要自动降级的场景请改用 [`LlmChainService`]。
    pub fn from_runtime(
        config: &AppRuntimeConfig,
        credential: &ResolvedCredential,
    ) -> Result<Self, AppError> {
        let link = config
            .llm_chain()
            .into_iter()
            .next()
            .ok_or_else(|| {
                if config.llm_configured() && !config.llm_active() {
                    AppError::configuration("大模型已停用，请先启用模型服务")
                } else {
                    AppError::configuration("请先配置大模型服务")
                }
            })?;
        Ok(Self::from_chain_link(&link, credential)?.with_retry(config.llm_retry_config.clone()))
    }

    /// 按降级链中的某一环构建服务。链首位与备用条目共用这一段 provider → backend 的构建逻辑。
    pub fn from_chain_link(
        link: &LlmChainLink,
        credential: &ResolvedCredential,
    ) -> Result<Self, AppError> {
        let provider = link.provider.clone();
        let base_url = normalize_provider_base_url(&provider, &link.base_url);
        let model = link.model.trim().to_string();
        if base_url.is_empty() || model.is_empty() {
            return Err(AppError::configuration("大模型地址和模型名称不能为空"));
        }

        let secret = credential.secret();
        if provider_requires_key(&provider) && secret.is_none() {
            return Err(AppError::credential("请先配置该大模型服务的 API Key"));
        }
        let api_key = secret.unwrap_or(if matches!(provider, LlmProviderPreset::Ollama) {
            ""
        } else {
            "noop"
        });

        let backend = match provider {
            LlmProviderPreset::Anthropic => LlmBackend::Anthropic(
                rig::providers::anthropic::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::DeepSeek => LlmBackend::DeepSeek(
                rig::providers::deepseek::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::OpenAi => {
                // Use Rig's OpenAI-compatible Chat Completions client so user supplied
                // OpenAI-compatible BaseURL values keep working with /chat/completions.
                LlmBackend::OpenAiCompatible(
                    rig::providers::openai::CompletionsClient::builder()
                        .api_key(api_key)
                        .base_url(&base_url)
                        .build()
                        .map_err(|e| {
                            AppError::configuration("无法创建大模型客户端")
                                .with_detail(e.to_string())
                        })?,
                )
            }
            LlmProviderPreset::OpenAiResponses => LlmBackend::OpenAiResponses(
                rig::providers::openai::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::MiniMax => LlmBackend::MiniMax(
                rig::providers::minimax::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::Moonshot => LlmBackend::Moonshot(
                rig::providers::moonshot::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::Ollama => LlmBackend::Ollama(
                rig::providers::ollama::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::OpenRouter => LlmBackend::OpenRouter(
                rig::providers::openrouter::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::XiaomiMimo => LlmBackend::XiaomiMimo(
                rig::providers::xiaomimimo::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::ZAi => LlmBackend::ZAi(
                rig::providers::zai::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
        };

        let display_name = link.display_name();

        Ok(Self {
            backend,
            model,
            provider,
            retry: LlmRetryConfig::default(),
            display_name,
        })
    }

    /// 覆盖默认重试策略（由 `AppRuntimeConfig::llm_retry_config` 提供）
    pub fn with_retry(mut self, retry: LlmRetryConfig) -> Self {
        self.retry = retry;
        self
    }

    pub async fn generate(&self, prompt: String) -> Result<LlmResponse, AppError> {
        match &self.backend {
            LlmBackend::Anthropic(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &self.retry,
                    &self.display_name,
                )
                .await
            }
            LlmBackend::DeepSeek(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &self.retry,
                    &self.display_name,
                )
                .await
            }
            LlmBackend::OpenAiCompatible(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &self.retry,
                    &self.display_name,
                )
                .await
            }
            LlmBackend::OpenAiResponses(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &self.retry,
                    &self.display_name,
                )
                .await
            }
            LlmBackend::MiniMax(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &self.retry,
                    &self.display_name,
                )
                .await
            }
            LlmBackend::Moonshot(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &self.retry,
                    &self.display_name,
                )
                .await
            }
            LlmBackend::Ollama(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &self.retry,
                    &self.display_name,
                )
                .await
            }
            LlmBackend::OpenRouter(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &self.retry,
                    &self.display_name,
                )
                .await
            }
            LlmBackend::XiaomiMimo(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &self.retry,
                    &self.display_name,
                )
                .await
            }
            LlmBackend::ZAi(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &self.retry,
                    &self.display_name,
                )
                .await
            }
        }
    }

    /// 流式采集：增量只在内部累积成完整文本，**一个字都不向外推送**。
    ///
    /// 正因为不外推，这条路径可以安全重试与降级——和 [`LlmService::stream`] 的约束正好相反
    /// （那条路径的增量已经落到界面上，重发会让用户看到重复或前后矛盾的内容）。
    ///
    /// 相比 [`LlmService::generate`]，流式在长输出上更稳：国产网关和反向代理常把
    /// 静默的长连接掐断，而流式全程有数据流动；模型陷在推理标签里不停输出时，
    /// 也不必等满整体超时才发现。
    ///
    /// **后来者注意**：若要在这里接 UI 增量推送，必须同时去掉重试，否则重发的内容会叠加。
    pub async fn stream_collect(&self, prompt: String) -> Result<LlmResponse, AppError> {
        let model_name = self.model.as_str();
        let provider = &self.provider;
        let retry = &self.retry;
        let service_name = self.display_name.as_str();
        let prompt = prompt.as_str();

        // 十个 provider 的采集逻辑只有 backend 变体不同，展开写会让每次新增 provider 都要改一大片
        macro_rules! collect {
            ($client:expr) => {
                stream_collect_with_retry(
                    || $client.completion_model(model_name),
                    model_name,
                    prompt,
                    provider,
                    retry,
                    service_name,
                )
                .await
            };
        }

        match &self.backend {
            LlmBackend::Anthropic(client) => collect!(client),
            LlmBackend::DeepSeek(client) => collect!(client),
            LlmBackend::OpenAiCompatible(client) => collect!(client),
            LlmBackend::OpenAiResponses(client) => collect!(client),
            LlmBackend::MiniMax(client) => collect!(client),
            LlmBackend::Moonshot(client) => collect!(client),
            LlmBackend::Ollama(client) => collect!(client),
            LlmBackend::OpenRouter(client) => collect!(client),
            LlmBackend::XiaomiMimo(client) => collect!(client),
            LlmBackend::ZAi(client) => collect!(client),
        }
    }

    /// 流式生成。**不做重试、也不参与降级链**：失败前可能已经向界面推送过增量文本，
    /// 换一个服务重发会让用户看到重复或前后矛盾的内容，所以只能原样把错误抛给调用方。
    ///
    /// 只要完整文本、不需要边生成边展示时，请改用 [`LlmService::stream_collect`]。
    pub async fn stream<F>(&self, prompt: String, mut on_delta: F) -> Result<LlmResponse, AppError>
    where
        F: FnMut(String) -> Result<(), AppError>,
    {
        let retry = &self.retry;

        macro_rules! stream {
            ($client:expr) => {
                stream_once(
                    $client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    retry,
                    &mut on_delta,
                )
                .await
            };
        }

        match &self.backend {
            LlmBackend::Anthropic(client) => stream!(client),
            LlmBackend::DeepSeek(client) => stream!(client),
            LlmBackend::OpenAiCompatible(client) => stream!(client),
            LlmBackend::OpenAiResponses(client) => stream!(client),
            LlmBackend::MiniMax(client) => stream!(client),
            LlmBackend::Moonshot(client) => stream!(client),
            LlmBackend::Ollama(client) => stream!(client),
            LlmBackend::OpenRouter(client) => stream!(client),
            LlmBackend::XiaomiMimo(client) => stream!(client),
            LlmBackend::ZAi(client) => stream!(client),
        }
    }

    pub async fn test_connection(&self) -> Result<ConnectionReport, AppError> {
        let response = self.generate("Reply with OK only.".to_string()).await?;
        Ok(ConnectionReport {
            model: response.model.unwrap_or_else(|| self.model.clone()),
            response: response.content,
        })
    }
}

/// 带降级能力的大模型服务：按 `AppRuntimeConfig::llm_chain()` 的顺序逐个尝试，
/// 任意一环成功即返回，全链失败才向调用方报错。
///
/// 只覆盖非流式调用。流式调用（模拟面试等）仍直接使用 [`LlmService`]：
/// 失败前增量文本可能已经推送到界面，换服务重发会造成内容重复。
#[derive(Debug, Clone)]
pub struct LlmChainService {
    links: Vec<LlmChainLink>,
    retry: LlmRetryConfig,
}

impl LlmChainService {
    pub fn from_runtime(config: &AppRuntimeConfig) -> Result<Self, AppError> {
        let links = config.llm_chain();
        if links.is_empty() {
            return Err(if config.llm_configured() && !config.llm_active() {
                AppError::configuration("大模型已停用，请先启用模型服务")
            } else {
                AppError::configuration("请先配置大模型服务")
            });
        }
        Ok(Self {
            links,
            retry: config.llm_retry_config.clone(),
        })
    }

    /// 链上服务的数量，仅用于日志与测试
    pub fn len(&self) -> usize {
        self.links.len()
    }

    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// 流式采集 + 降级链。语义见 [`LlmService::stream_collect`]：增量不外推，因此可以安全重试。
    ///
    /// Agent 循环统一走这条路径，不再直接用 [`LlmChainService::generate`]。
    pub async fn stream_collect(&self, prompt: String) -> Result<LlmResponse, AppError> {
        let retry = self.retry.clone();
        run_with_chain(&self.links, move |link| {
            let retry = retry.clone();
            let prompt = prompt.clone();
            async move {
                let credential = crate::credential::resolve_for_entry(&link.id)?;
                let service = LlmService::from_chain_link(link, &credential)?.with_retry(retry);
                service.stream_collect(prompt).await
            }
        })
        .await
    }

    /// 流式生成 + 有条件的降级链。增量通过 `on_delta` 实时推给调用方。
    ///
    /// 降级只在**一个字都还没吐出去**的时候允许：一旦开始推增量，界面上已经有内容了，
    /// 换一个服务重发会让用户看到重复或前后矛盾的文本。所以这里不复用
    /// [`run_with_chain`]——那套无条件往下试，正是这个场景不能接受的。
    ///
    /// 也因此不做重试：重试和降级在这里是同一个问题。
    pub async fn stream_with<F>(
        &self,
        prompt: String,
        mut on_delta: F,
    ) -> Result<LlmResponse, AppError>
    where
        F: FnMut(String) -> Result<(), AppError>,
    {
        let total = self.links.len();
        let mut emitted = false;
        let mut last_error: Option<AppError> = None;

        for (index, link) in self.links.iter().enumerate() {
            let attempt = async {
                let credential = crate::credential::resolve_for_entry(&link.id)?;
                let service = LlmService::from_chain_link(link, &credential)?;
                service
                    .stream(prompt.clone(), |delta| {
                        emitted = true;
                        on_delta(delta)
                    })
                    .await
            }
            .await;

            match attempt {
                Ok(response) => {
                    if index > 0 {
                        let _ = crate::logger::info(format!(
                            "主用大模型服务不可用，已降级到备用服务「{}」",
                            link.display_name()
                        ));
                    }
                    return Ok(response);
                }
                Err(error) => {
                    if emitted {
                        // 已经有内容推到界面上了，只能把错误原样抛出去
                        return Err(error);
                    }
                    if total > 1 {
                        let _ = crate::logger::warning(format!(
                            "大模型服务「{}」流式调用失败：{}，尝试下一个备用服务",
                            link.display_name(),
                            error.message
                        ));
                    }
                    last_error = Some(error);
                }
            }
        }

        Err(chain_exhausted_error(last_error, total))
    }
}

/// 按降级链顺序执行 `attempt`，成功即返回；把控制流与真实网络请求解耦，便于测试。
///
/// 降级规则：**所有类型的失败都触发降级**（密钥缺失、配置无效、鉴权失败、额度受限、
/// 超时、网络故障）。降级链存在的意义就是主用服务不可用时兜底，没必要区分不可用的原因。
pub(crate) async fn run_with_chain<'a, F, Fut>(
    links: &'a [LlmChainLink],
    mut attempt: F,
) -> Result<LlmResponse, AppError>
where
    F: FnMut(&'a LlmChainLink) -> Fut,
    Fut: Future<Output = Result<LlmResponse, AppError>>,
{
    let total = links.len();
    let mut last_error: Option<AppError> = None;

    for (index, link) in links.iter().enumerate() {
        match attempt(link).await {
            Ok(response) => {
                // 只有单环时行为与改动前完全一致：不打任何降级日志
                if index > 0 {
                    let _ = crate::logger::info(format!(
                        "主用大模型服务不可用，已降级到备用服务「{}」",
                        link.display_name()
                    ));
                }
                return Ok(response);
            }
            Err(error) => {
                if total > 1 {
                    let remaining = total - index - 1;
                    let next_hint = if remaining > 0 {
                        format!("，继续尝试下一个备用服务（还剩 {remaining} 个）")
                    } else {
                        "，降级链已耗尽".to_string()
                    };
                    let _ = crate::logger::warning(format!(
                        "大模型服务「{}」调用失败：{}{}",
                        link.display_name(),
                        error.message,
                        next_hint
                    ));
                }
                last_error = Some(error);
            }
        }
    }

    Err(chain_exhausted_error(last_error, total))
}

/// 全链耗尽时返回**最后一个**失败的错误。
/// 多环时在消息里说明降级发生过；单环时保持原始消息不变，避免给绝大多数用户增加噪音。
pub(crate) fn chain_exhausted_error(last_error: Option<AppError>, attempted: usize) -> AppError {
    let Some(error) = last_error else {
        return AppError::configuration("请先配置大模型服务");
    };
    if attempted <= 1 {
        return error;
    }
    AppError {
        message: format!(
            "已依次尝试 {attempted} 个大模型服务均失败：{}",
            error.message
        ),
        ..error
    }
}

/// 单次请求的失败来源。**超时**与**请求返回错误**必须结构性分开：
/// 超时意味着配置的等待时间已经耗尽，重试只会成倍拖慢整轮求职；
/// 只有「请求返回错误」里的网络层瞬时故障才值得重试。
#[derive(Debug)]
pub(crate) enum AttemptFailure {
    /// `tokio::time::timeout` 触发的整体超时
    Timeout,
    /// 请求返回了错误，已由 `map_completion_error` 映射成 `AppError`
    Completion(AppError),
}

impl AttemptFailure {
    /// 日志里展示的失败原因
    pub(crate) fn reason(&self) -> &str {
        match self {
            Self::Timeout => "大模型请求超时",
            Self::Completion(error) => error.message.as_str(),
        }
    }

    pub(crate) fn into_error(self) -> AppError {
        match self {
            Self::Timeout => AppError::network("大模型请求超时"),
            Self::Completion(error) => error,
        }
    }
}

/// 判断本次失败是否还要重试，要重试则返回退避时长。
/// `attempt` 为已完成的重试次数（首次请求失败时为 0）。
pub(crate) fn retry_plan(
    failure: &AttemptFailure,
    attempt: u32,
    retry: &LlmRetryConfig,
) -> Option<Duration> {
    if attempt >= retry.network_retry_attempts {
        return None;
    }
    match failure {
        // 超时不重试：请求已经等满，重发只会让整轮求职成倍变慢
        AttemptFailure::Timeout => None,
        // 只对网络层瞬时故障重试；鉴权失败、额度受限、配置错误重试多少次都是同样结果
        AttemptFailure::Completion(error) => (error.code == AppErrorCode::Network)
            .then(|| retry_backoff_delay(retry.retry_base_delay_ms, attempt)),
    }
}

/// 指数退避：第 n 次重试前等待 `base_delay_ms * 2^n` 毫秒
pub(crate) fn retry_backoff_delay(base_delay_ms: u64, attempt: u32) -> Duration {
    let factor = 1_u64.checked_shl(attempt).unwrap_or(u64::MAX);
    Duration::from_millis(base_delay_ms.saturating_mul(factor))
}

async fn complete_once<M>(
    model: M,
    model_name: &str,
    prompt: &str,
    provider: &LlmProviderPreset,
    retry: &LlmRetryConfig,
    service_name: &str,
) -> Result<LlmResponse, AppError>
where
    M: CompletionModel + Send,
{
    // 已完成的重试次数；`retry.network_retry_attempts` 为 0 时循环只会执行一遍
    let mut attempt: u32 = 0;
    loop {
        let outcome = timeout(Duration::from_secs(retry.request_timeout_seconds), async {
            model.completion_request(prompt).send().await
        })
        .await;

        let failure = match outcome {
            // 超时分支直接返回，永不进入重试判定
            Err(_) => return Err(AttemptFailure::Timeout.into_error()),
            Ok(Ok(response)) => {
                let mut content = String::new();
                for item in response.choice {
                    if let rig::completion::AssistantContent::Text(text) = item {
                        content.push_str(&text.text);
                    }
                }

                return Ok(LlmResponse {
                    content,
                    model: Some(model_name.to_string()),
                    finish_reason: None,
                    usage: None,
                });
            }
            // 只有请求返回错误的分支才可能重试
            Ok(Err(completion_error)) => {
                AttemptFailure::Completion(map_completion_error(completion_error, provider))
            }
        };

        let Some(delay) = retry_plan(&failure, attempt, retry) else {
            return Err(failure.into_error());
        };

        // 日志失败不能影响主流程
        let _ = crate::logger::warning(format!(
            "大模型服务「{service_name}」网络异常，{} 毫秒后重试（第 {}/{} 次）：{}",
            delay.as_millis(),
            attempt + 1,
            retry.network_retry_attempts,
            failure.reason()
        ));
        tokio::time::sleep(delay).await;
        attempt += 1;
    }
}

/// 流式请求刻意不做重试：错误发生前增量文本可能已经推送到界面，
/// 重发会导致内容重复。所以这里维持「一次失败即返回」的语义。
async fn stream_once<M, F>(
    model: M,
    model_name: &str,
    prompt: &str,
    provider: &LlmProviderPreset,
    retry: &LlmRetryConfig,
    on_delta: &mut F,
) -> Result<LlmResponse, AppError>
where
    M: CompletionModel + Send,
    F: FnMut(String) -> Result<(), AppError>,
{
    timeout(Duration::from_secs(retry.request_timeout_seconds), async {
        let request = model.completion_request(prompt).build();
        let mut stream = model
            .stream(request)
            .await
            .map_err(|error| map_stream_completion_error(error, provider))?;

        let mut content = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| map_stream_completion_error(error, provider))?;
            if let StreamedAssistantContent::Text(text) = chunk {
                content.push_str(&text.text);
                on_delta(text.text)?;
            }
        }

        Ok(LlmResponse {
            content,
            model: Some(model_name.to_string()),
            finish_reason: None,
            usage: None,
        })
    })
    .await
    .map_err(|_| AppError::network("大模型流式请求超时"))?
}

/// 流式采集一次，把增量攒成完整文本。失败原因区分超时与请求错误，交给 [`retry_plan`] 判定。
///
/// 这里刻意不复用 [`stream_once`]：那个函数把超时也映射成 `AppError::network`，
/// 与「连不上」无法区分，而超时按既定策略是绝不重试的。
async fn stream_collect_attempt<M>(
    model: M,
    model_name: &str,
    prompt: &str,
    provider: &LlmProviderPreset,
    retry: &LlmRetryConfig,
) -> Result<LlmResponse, AttemptFailure>
where
    M: CompletionModel + Send,
{
    let outcome = timeout(Duration::from_secs(retry.request_timeout_seconds), async {
        let request = model.completion_request(prompt).build();
        let mut stream = model
            .stream(request)
            .await
            .map_err(|error| map_stream_completion_error(error, provider))?;

        let mut content = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| map_stream_completion_error(error, provider))?;
            if let StreamedAssistantContent::Text(text) = chunk {
                content.push_str(&text.text);
            }
        }

        Ok::<_, AppError>(LlmResponse {
            content,
            model: Some(model_name.to_string()),
            finish_reason: None,
            usage: None,
        })
    })
    .await;

    match outcome {
        Err(_) => Err(AttemptFailure::Timeout),
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Err(AttemptFailure::Completion(error)),
    }
}

/// 给流式采集套上与 [`complete_once`] 相同的重试策略。
///
/// 模型按 `make_model` 每次重新构建：流断在中途时已收到的增量会被整段丢弃重来，
/// 这正是「增量不外推」换来的自由——外部永远只看到成功那一次的完整结果。
async fn stream_collect_with_retry<M, F>(
    make_model: F,
    model_name: &str,
    prompt: &str,
    provider: &LlmProviderPreset,
    retry: &LlmRetryConfig,
    service_name: &str,
) -> Result<LlmResponse, AppError>
where
    M: CompletionModel + Send,
    F: Fn() -> M,
{
    let mut attempt: u32 = 0;
    loop {
        let failure = match stream_collect_attempt(make_model(), model_name, prompt, provider, retry).await
        {
            Ok(response) => return Ok(response),
            Err(failure) => failure,
        };

        let Some(delay) = retry_plan(&failure, attempt, retry) else {
            return Err(failure.into_error());
        };

        // 日志失败不能影响主流程
        let _ = crate::logger::warning(format!(
            "大模型服务「{service_name}」流式采集网络异常，{} 毫秒后重试（第 {}/{} 次）：{}",
            delay.as_millis(),
            attempt + 1,
            retry.network_retry_attempts,
            failure.reason()
        ));
        tokio::time::sleep(delay).await;
        attempt += 1;
    }
}

pub(crate) fn normalize_provider_base_url(provider: &LlmProviderPreset, base_url: &str) -> String {
    let base_url = base_url.trim().trim_end_matches('/');
    if matches!(provider, LlmProviderPreset::Ollama) {
        base_url.strip_suffix("/v1").unwrap_or(base_url).to_string()
    } else {
        base_url.to_string()
    }
}

pub(crate) fn provider_requires_key(provider: &LlmProviderPreset) -> bool {
    !matches!(provider, LlmProviderPreset::Ollama)
}

fn map_completion_error(error: CompletionError, provider: &LlmProviderPreset) -> AppError {
    map_rig_error(error, false, provider)
}

fn map_stream_completion_error(error: CompletionError, provider: &LlmProviderPreset) -> AppError {
    map_rig_error(error, true, provider)
}

fn map_rig_error(
    error: CompletionError,
    streaming: bool,
    provider: &LlmProviderPreset,
) -> AppError {
    let status_code = error
        .provider_response_status()
        .map(|status| status.as_u16());
    let metadata = error
        .provider_response_json()
        .ok()
        .flatten()
        .and_then(|value| value.get("error").cloned())
        .and_then(|value| provider_error_metadata(&value));

    // Build a safe user-facing diagnostic (HTTP status + provider error code/type).
    // The `provider_error_metadata` output only includes alphanumeric-safe fields
    // (code, type), never raw message bodies, so it is safe to surface to users.
    let diagnostic = match (status_code, metadata.as_ref()) {
        (Some(s), Some(m)) => Some(format!("（HTTP {s}，{m}）")),
        (Some(s), None) => Some(format!("（HTTP {s}）")),
        (None, _) => None,
    };

    let base = match status_code {
        Some(401 | 403) => "大模型密钥无效或无权访问",
        Some(404) if matches!(provider, LlmProviderPreset::Ollama) => {
            "Ollama 模型不存在，请先在 Ollama 中 pull 该模型"
        }
        Some(404) => "大模型地址或模型不存在",
        Some(429) => "大模型服务返回 HTTP 429：请求受限或账户额度不足",
        Some(_) => "大模型服务请求失败",
        None => match &error {
            CompletionError::HttpError(_) => {
                if matches!(provider, LlmProviderPreset::Ollama) {
                    "无法连接 Ollama，请确认 Ollama 已启动"
                } else if streaming {
                    "大模型流式请求失败"
                } else {
                    "无法连接大模型服务"
                }
            }
            _ => {
                if streaming {
                    "大模型流式生成失败"
                } else {
                    "大模型生成失败"
                }
            }
        },
    };

    let message = match diagnostic.as_deref() {
        Some(d) => format!("{base}{d}"),
        None => base.to_string(),
    };

    let mut mapped = match status_code {
        Some(401 | 403) => AppError::credential(message),
        Some(404 | 429) => AppError::provider(message),
        Some(_) => AppError::provider(message),
        None => match &error {
            CompletionError::HttpError(_) => AppError::network(message),
            _ => AppError::provider(message),
        },
    };

    // Attach full diagnostics as internal detail for logging (never serialized).
    let detail = match (status_code, metadata) {
        (Some(s), Some(m)) => Some(format!("HTTP {s}; {m}")),
        (Some(s), None) => Some(format!("HTTP {s}")),
        (None, _) => safe_completion_detail(&error),
    };
    if let Some(detail) = detail {
        mapped = mapped.with_detail(detail);
    }
    mapped
}

fn safe_completion_detail(error: &CompletionError) -> Option<String> {
    match error {
        CompletionError::HttpError(rig::http_client::Error::Instance(source)) => {
            let message = source.to_string().to_ascii_lowercase();
            if message.contains("timeout") || message.contains("timed out") {
                Some("request timed out".to_string())
            } else {
                Some("HTTP transport failed".to_string())
            }
        }
        CompletionError::HttpError(_) => Some("HTTP transport failed".to_string()),
        CompletionError::JsonError(error) => Some(format!(
            "JSON response parse error: category={:?}, line={}, column={}",
            error.classify(),
            error.line(),
            error.column()
        )),
        CompletionError::UrlError(_) => Some("invalid provider URL".to_string()),
        CompletionError::ResponseError(_) => Some("provider response parse failed".to_string()),
        CompletionError::ProviderResponse(_) => {
            Some("provider returned an error response".to_string())
        }
        CompletionError::ProviderError(_) => Some("provider request failed".to_string()),
        CompletionError::RequestError(_) => Some("completion request build failed".to_string()),
        _ => None,
    }
}

fn safe_provider_identifier(value: &str) -> Option<&str> {
    (!value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
    .then_some(value)
}

fn provider_error_metadata(error: &Value) -> Option<String> {
    let mut fields = Vec::new();
    for name in ["code", "type"] {
        if let Some(value) = error
            .get(name)
            .and_then(Value::as_str)
            .and_then(safe_provider_identifier)
        {
            fields.push(format!("{name}={value}"));
        }
    }
    (!fields.is_empty()).then(|| fields.join(", "))
}

#[cfg(test)]
mod tests {
    use super::{AttemptFailure, LlmChainService, LlmService};
    use crate::config::{
        default_app_config, LlmChainLink, LlmConfig, LlmProviderEntry, LlmProviderPreset,
        LlmRetryConfig, PRIMARY_LLM_ENTRY_ID,
    };
    use crate::credential::{resolve_with_environment, CredentialBackend};
    use crate::error::{AppError, AppErrorCode};
    use crate::llm::types::LlmResponse;
    use serde_json::Value;
    use std::cell::{Cell, RefCell};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;

    #[derive(Default)]
    struct EmptyCredentialBackend;

    impl CredentialBackend for EmptyCredentialBackend {
        fn get(&self) -> Result<Option<String>, AppError> {
            Ok(None)
        }

        fn set(&self, _secret: &str) -> Result<(), AppError> {
            Ok(())
        }

        fn delete(&self) -> Result<(), AppError> {
            Ok(())
        }
    }

    fn service(base_url: String) -> LlmService {
        service_with_provider(LlmProviderPreset::OpenAi, base_url)
    }

    fn service_with_provider(provider: LlmProviderPreset, base_url: String) -> LlmService {
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider,
            base_url,
            model: "local-model".to_string(),
        });
        let credential = resolve_with_environment(&EmptyCredentialBackend, Some("secret")).unwrap();
        LlmService::from_runtime(&config, &credential).unwrap()
    }

    fn mock_server(response_parts: Vec<&'static [u8]>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(header_end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..header_end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&bytes).to_string());
            for part in response_parts {
                stream.write_all(part).unwrap();
                stream.flush().unwrap();
            }
        });
        (format!("http://{address}/v1"), receiver)
    }

    fn http_response(content_type: &str, body: &str) -> &'static [u8] {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        Box::leak(response.into_bytes().into_boxed_slice())
    }

    #[test]
    fn rig_completion_uses_chat_completions_with_auth_and_model() {
        let body = r#"{"id":"chatcmpl-1","object":"chat.completion","created":1,"model":"local-model","system_fingerprint":null,"choices":[{"index":0,"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2,"prompt_tokens_details":null,"completion_tokens_details":null}}"#;
        let (url, received) = mock_server(vec![http_response("application/json", body)]);
        let result = tauri::async_runtime::block_on(service(url).test_connection()).unwrap();

        assert_eq!(result.response, "OK");
        let raw = received.recv().unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /v1/chat/completions "));
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer secret"));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["model"], "local-model");
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("max_tokens").is_none());
        assert!(payload.get("stream").is_none());
    }

    #[test]
    fn rig_responses_provider_uses_responses_endpoint_with_auth_and_model() {
        let body = r#"{"id":"resp_1","object":"response","created_at":1,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"local-model","usage":null,"output":[{"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"OK","annotations":[]}]}],"tools":[]}"#;
        let (url, received) = mock_server(vec![http_response("application/json", body)]);
        let result = tauri::async_runtime::block_on(
            service_with_provider(LlmProviderPreset::OpenAiResponses, url).test_connection(),
        )
        .unwrap();

        assert_eq!(result.response, "OK");
        let raw = received.recv().unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /v1/responses "));
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer secret"));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["model"], "local-model");
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("max_output_tokens").is_none());
        assert!(payload.get("stream").is_none());
    }

    #[test]
    fn rig_responses_stream_uses_responses_endpoint_and_preserves_delta_order() {
        let completed = r#"{"type":"response.completed","sequence_number":3,"response":{"id":"resp_1","object":"response","created_at":1,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"local-model","usage":null,"output":[{"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"OK","annotations":[]}]}],"tools":[]}}"#;
        let (url, received) = mock_server(vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"sequence_number\":1,\"delta\":\"O\"}\n\n",
            b"data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"sequence_number\":2,\"delta\":\"K\"}\n\n",
            Box::leak(format!("data: {completed}\n\ndata: [DONE]\n\n").into_bytes().into_boxed_slice()),
        ]);
        let mut deltas = Vec::new();
        let report = tauri::async_runtime::block_on(
            service_with_provider(LlmProviderPreset::OpenAiResponses, url).stream(
                "stream test".to_string(),
                |delta| {
                    deltas.push(delta);
                    Ok(())
                },
            ),
        )
        .unwrap();

        assert_eq!(report.content, "OK");
        assert_eq!(deltas, ["O", "K"]);
        let raw = received.recv().unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /v1/responses "));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["stream"], true);
    }

    #[test]
    fn rig_stream_uses_chat_completions_and_preserves_delta_order() {
        let (url, received) = mock_server(vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"id\":\"chatcmpl-1\",\"model\":\"local-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"O\"},\"finish_reason\":null}]}\n\n",
            b"data: {\"id\":\"chatcmpl-1\",\"model\":\"local-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"K\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ]);
        let mut deltas = Vec::new();
        let report = tauri::async_runtime::block_on(service(url).stream(
            "stream test".to_string(),
            |delta| {
                deltas.push(delta);
                Ok(())
            },
        ))
        .unwrap();

        assert_eq!(report.content, "OK");
        assert_eq!(deltas, ["O", "K"]);
        let raw = received.recv().unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /v1/chat/completions "));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["stream"], true);
    }

    #[test]
    fn rig_provider_error_maps_status_without_retaining_echoed_body() {
        let secret = "prompt-and-token-never-retain";
        let body = format!(
            r#"{{"error":{{"message":"{secret}","code":"rate_limit","type":"requests"}}}}"#
        );
        let response = format!(
            "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let leaked = Box::leak(response.into_bytes().into_boxed_slice());
        let (url, _) = mock_server(vec![leaked]);

        let error = tauri::async_runtime::block_on(service(url).test_connection()).unwrap_err();
        assert!(
            error
                .message
                .contains("大模型服务返回 HTTP 429：请求受限或账户额度不足"),
            "message should include the base error: {}",
            error.message
        );
        assert!(
            error.message.contains("（HTTP 429") && error.message.contains("code=rate_limit"),
            "message should include safe provider diagnostics: {}",
            error.message
        );
        assert!(
            !error.message.contains(secret),
            "message must not echo the provider body"
        );
        let detail = error.detail.unwrap_or_default();
        assert!(detail.contains("HTTP 429"));
        assert!(detail.contains("code=rate_limit"));
        assert!(!detail.contains(secret));
    }

    #[test]
    fn ollama_provider_uses_native_chat_endpoint_and_strips_legacy_v1_suffix() {
        let body = r#"{"model":"llama3.2","created_at":"2026-08-10T00:00:00Z","message":{"role":"assistant","content":"OK"},"done":true,"prompt_eval_count":1,"eval_count":1}"#;
        let (url, received) = mock_server(vec![http_response("application/json", body)]);
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderPreset::Ollama,
            base_url: url,
            model: "llama3.2".to_string(),
        });
        let credential = resolve_with_environment(&EmptyCredentialBackend, None).unwrap();
        let result = tauri::async_runtime::block_on(
            LlmService::from_runtime(&config, &credential)
                .unwrap()
                .test_connection(),
        )
        .unwrap();

        assert_eq!(result.response, "OK");
        let raw = received.recv().unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /api/chat "));
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["model"], "llama3.2");
        assert_eq!(payload["stream"], false);
    }

    #[test]
    fn key_required_providers_fail_before_client_creation_without_secret() {
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderPreset::DeepSeek,
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
        });
        let credential = resolve_with_environment(&EmptyCredentialBackend, None).unwrap();
        let error = LlmService::from_runtime(&config, &credential).unwrap_err();

        assert_eq!(error.message, "请先配置该大模型服务的 API Key");
    }

    // ================================
    // 重试判定与退避（纯函数，不依赖真实网络）
    // ================================

    fn retry_config(attempts: u32, base_delay_ms: u64) -> LlmRetryConfig {
        LlmRetryConfig {
            network_retry_attempts: attempts,
            retry_base_delay_ms: base_delay_ms,
            request_timeout_seconds: 120,
        }
    }

    #[test]
    fn network_failures_are_retryable() {
        let retry = retry_config(2, 500);
        let failure = AttemptFailure::Completion(AppError::network("无法连接大模型服务"));

        assert_eq!(
            super::retry_plan(&failure, 0, &retry),
            Some(Duration::from_millis(500))
        );
    }

    #[test]
    fn credential_quota_and_configuration_failures_are_not_retryable() {
        let retry = retry_config(3, 500);
        let cases = [
            AppError::credential("大模型密钥无效或无权访问"),
            AppError::provider("大模型服务返回 HTTP 429：请求受限或账户额度不足"),
            AppError::provider("大模型地址或模型不存在"),
            AppError::configuration("大模型地址和模型名称不能为空"),
        ];

        for error in cases {
            let message = error.message.clone();
            let failure = AttemptFailure::Completion(error);
            assert_eq!(
                super::retry_plan(&failure, 0, &retry),
                None,
                "不该重试：{message}"
            );
        }
    }

    #[test]
    fn request_timeout_is_never_retried() {
        let retry = retry_config(5, 500);

        // 120 秒已经等满，任何一次重试前都不该再等
        for attempt in 0..5 {
            assert_eq!(
                super::retry_plan(&AttemptFailure::Timeout, attempt, &retry),
                None
            );
        }
        assert_eq!(
            AttemptFailure::Timeout.into_error(),
            AppError::network("大模型请求超时")
        );
    }

    #[test]
    fn backoff_doubles_from_the_base_delay() {
        assert_eq!(
            super::retry_backoff_delay(500, 0),
            Duration::from_millis(500)
        );
        assert_eq!(
            super::retry_backoff_delay(500, 1),
            Duration::from_millis(1_000)
        );
        assert_eq!(
            super::retry_backoff_delay(500, 2),
            Duration::from_millis(2_000)
        );

        let retry = retry_config(3, 500);
        let delays: Vec<_> = (0..3)
            .map(|attempt| {
                super::retry_plan(
                    &AttemptFailure::Completion(AppError::network("无法连接大模型服务")),
                    attempt,
                    &retry,
                )
            })
            .collect();
        assert_eq!(
            delays,
            vec![
                Some(Duration::from_millis(500)),
                Some(Duration::from_millis(1_000)),
                Some(Duration::from_millis(2_000)),
            ]
        );
    }

    #[test]
    fn zero_retry_attempts_disables_retry_entirely() {
        let retry = retry_config(0, 500);
        let failure = AttemptFailure::Completion(AppError::network("无法连接大模型服务"));

        assert_eq!(super::retry_plan(&failure, 0, &retry), None);
    }

    #[test]
    fn retry_stops_after_the_configured_attempt_count() {
        let retry = retry_config(2, 500);
        let failure = AttemptFailure::Completion(AppError::network("无法连接大模型服务"));

        assert!(super::retry_plan(&failure, 0, &retry).is_some());
        assert!(super::retry_plan(&failure, 1, &retry).is_some());
        assert_eq!(super::retry_plan(&failure, 2, &retry), None);
    }

    // ================================
    // 降级链控制流（假实现，不依赖真实网络）
    // ================================

    fn chain_link(id: &str, label: Option<&str>) -> LlmChainLink {
        LlmChainLink {
            id: id.to_string(),
            label: label.map(str::to_string),
            provider: LlmProviderPreset::OpenAi,
            base_url: "http://127.0.0.1:1/v1".to_string(),
            model: format!("{id}-model"),
        }
    }

    fn ok_response(content: &str) -> LlmResponse {
        LlmResponse {
            content: content.to_string(),
            model: None,
            finish_reason: None,
            usage: None,
        }
    }

    #[test]
    fn chain_falls_back_to_the_next_link_when_the_primary_fails() {
        let links = vec![
            chain_link(PRIMARY_LLM_ENTRY_ID, None),
            chain_link("backup-1", Some("备用一号")),
        ];
        let attempted = RefCell::new(Vec::new());

        let response = tauri::async_runtime::block_on(super::run_with_chain(&links, |link| {
            attempted.borrow_mut().push(link.id.clone());
            let is_primary = link.is_primary();
            async move {
                if is_primary {
                    Err(AppError::network("无法连接大模型服务"))
                } else {
                    Ok(ok_response("备用服务的结果"))
                }
            }
        }))
        .unwrap();

        assert_eq!(response.content, "备用服务的结果");
        assert_eq!(
            attempted.into_inner(),
            vec![PRIMARY_LLM_ENTRY_ID.to_string(), "backup-1".to_string()]
        );
    }

    #[test]
    fn chain_stops_at_the_first_success_without_touching_fallbacks() {
        let links = vec![
            chain_link(PRIMARY_LLM_ENTRY_ID, None),
            chain_link("backup-1", None),
            chain_link("backup-2", None),
        ];
        let calls = Cell::new(0_usize);

        let response = tauri::async_runtime::block_on(super::run_with_chain(&links, |_link| {
            calls.set(calls.get() + 1);
            async move { Ok(ok_response("主用服务的结果")) }
        }))
        .unwrap();

        assert_eq!(response.content, "主用服务的结果");
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn every_failure_kind_triggers_fallback_including_missing_credentials() {
        let links = vec![
            chain_link(PRIMARY_LLM_ENTRY_ID, None),
            chain_link("backup-1", None),
            chain_link("backup-2", None),
        ];
        let calls = Cell::new(0_usize);

        let response = tauri::async_runtime::block_on(super::run_with_chain(&links, |_link| {
            let index = calls.get();
            calls.set(index + 1);
            async move {
                match index {
                    // 密钥缺失同样降级：链存在的意义就是主用服务不可用时兜底
                    0 => Err(AppError::credential("请先配置该大模型服务的 API Key")),
                    // 额度受限也降级
                    1 => Err(AppError::provider(
                        "大模型服务返回 HTTP 429：请求受限或账户额度不足",
                    )),
                    _ => Ok(ok_response("最后一环成功")),
                }
            }
        }))
        .unwrap();

        assert_eq!(response.content, "最后一环成功");
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn exhausted_chain_reports_the_number_of_attempted_services() {
        let links = vec![
            chain_link(PRIMARY_LLM_ENTRY_ID, None),
            chain_link("backup-1", None),
        ];
        let calls = Cell::new(0_usize);

        let error = tauri::async_runtime::block_on(super::run_with_chain(&links, |_link| {
            let index = calls.get();
            calls.set(index + 1);
            async move {
                if index == 0 {
                    Err(AppError::network("无法连接大模型服务"))
                } else {
                    // 全链耗尽时应返回最后一个失败的错误
                    Err(AppError::credential("大模型密钥无效或无权访问"))
                }
            }
        }))
        .unwrap_err();

        assert_eq!(calls.get(), 2);
        assert_eq!(error.code, AppErrorCode::Credential);
        assert!(
            error.message.contains("已依次尝试 2 个大模型服务均失败"),
            "应说明降级发生过：{}",
            error.message
        );
        assert!(error.message.contains("大模型密钥无效或无权访问"));
    }

    #[test]
    fn single_link_failure_keeps_the_original_message() {
        let links = vec![chain_link(PRIMARY_LLM_ENTRY_ID, None)];

        let error = tauri::async_runtime::block_on(super::run_with_chain(&links, |_link| async {
            Err(AppError::network("无法连接大模型服务"))
        }))
        .unwrap_err();

        // 绝大多数用户只有一环，行为必须与改动前完全一致
        assert_eq!(error.message, "无法连接大模型服务");
        assert!(!error.message.contains("依次尝试"));
        assert!(!error.message.contains("降级"));
    }

    #[test]
    fn chain_exhausted_error_only_annotates_multi_link_chains() {
        assert_eq!(
            super::chain_exhausted_error(Some(AppError::network("无法连接大模型服务")), 1).message,
            "无法连接大模型服务"
        );
        assert_eq!(
            super::chain_exhausted_error(Some(AppError::network("无法连接大模型服务")), 3).message,
            "已依次尝试 3 个大模型服务均失败：无法连接大模型服务"
        );
        assert_eq!(
            super::chain_exhausted_error(None, 0).code,
            AppErrorCode::Configuration
        );
    }

    // ================================
    // LlmChainService 装配
    // ================================

    #[test]
    fn chain_service_requires_a_configured_primary_service() {
        let mut config = default_app_config();
        config.llm_config = None;
        config.llm_fallbacks = vec![LlmProviderEntry {
            id: "backup-1".to_string(),
            label: None,
            provider: LlmProviderPreset::OpenAi,
            base_url: "http://127.0.0.1:1/v1".to_string(),
            model: "backup-model".to_string(),
            enabled: true,
        }];

        let error = LlmChainService::from_runtime(&config).unwrap_err();

        assert_eq!(error.code, AppErrorCode::Configuration);
        assert_eq!(error.message, "请先配置大模型服务");
    }

    #[test]
    fn chain_service_reports_a_disabled_primary_without_losing_its_configuration() {
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderPreset::OpenAi,
            base_url: "http://127.0.0.1:1/v1".to_string(),
            model: "primary-model".to_string(),
        });
        config.llm_enabled = Some(false);

        let error = LlmChainService::from_runtime(&config).unwrap_err();

        assert_eq!(error.code, AppErrorCode::Configuration);
        assert_eq!(error.message, "大模型已停用，请先启用模型服务");
        assert!(config.llm_config.is_some());
    }

    #[test]
    fn chain_service_keeps_primary_first_and_carries_retry_config() {
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderPreset::OpenAi,
            base_url: "http://127.0.0.1:1/v1".to_string(),
            model: "primary-model".to_string(),
        });
        config.llm_fallbacks = vec![LlmProviderEntry {
            id: "backup-1".to_string(),
            label: Some("备用一号".to_string()),
            provider: LlmProviderPreset::DeepSeek,
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
            enabled: true,
        }];
        config.llm_retry_config = retry_config(4, 800);

        let service = LlmChainService::from_runtime(&config).unwrap();

        assert_eq!(service.len(), 2);
        assert!(!service.is_empty());
        assert!(service.links[0].is_primary());
        assert_eq!(service.links[0].display_name(), "primary-model");
        assert_eq!(service.links[1].display_name(), "备用一号");
        assert_eq!(service.retry, retry_config(4, 800));
    }

    #[test]
    fn chain_link_service_inherits_the_configured_retry_policy() {
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderPreset::OpenAi,
            base_url: "http://127.0.0.1:1/v1".to_string(),
            model: "primary-model".to_string(),
        });
        config.llm_retry_config = retry_config(1, 250);
        let credential = resolve_with_environment(&EmptyCredentialBackend, Some("secret")).unwrap();

        let service = LlmService::from_runtime(&config, &credential).unwrap();

        assert_eq!(service.retry, retry_config(1, 250));
        assert_eq!(service.display_name, "primary-model");
    }

    #[test]
    fn normalizes_ollama_base_url_only_at_runtime() {
        assert_eq!(
            super::normalize_provider_base_url(
                &LlmProviderPreset::Ollama,
                " http://localhost:11434/v1/ "
            ),
            "http://localhost:11434"
        );
        assert_eq!(
            super::normalize_provider_base_url(
                &LlmProviderPreset::OpenAi,
                " http://localhost:1234/v1/ "
            ),
            "http://localhost:1234/v1"
        );
    }
}
