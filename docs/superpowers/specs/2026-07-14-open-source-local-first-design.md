# 开源本地版设计规格

**日期：** 2026-07-14  
**状态：** 已确认，待实施  
**目标版本：** 首个开源本地版

## 1. 背景与目标

当前应用是 React + Tauri 桌面应用。岗位、聊天、分析、简历和运行配置已经主要保存在本机，但启动、账户、积分、大模型生成和自动更新仍依赖原项目服务器 `fk.pgthinker.me`。岗位分析还会直接调用固定的 DashScope 联网搜索接口。

首个开源版本改为 local-first 桌面应用：

- 不登录、不注册，不需要原作者提供任何在线业务服务。
- 不访问原项目服务器，不包含遥测、埋点或自动更新请求。
- 岗位、聊天、简历、分析和配置保存在用户本机。
- Boss 直聘和猎聘自动化继续访问对应招聘平台。
- 所有 AI 功能直接使用用户配置的 OpenAI-compatible 服务。
- 支持在线模型服务，也支持 Ollama、LM Studio 等本地服务。
- 以首次使用顺畅为优先级，允许对现有架构做系统性调整。

## 2. 第一版范围

### 2.1 包含

- 删除原服务端鉴权、账户、积分、充值和更新体系。
- 建立统一的本地 LLM Provider，支持普通生成和 SSE 流式生成。
- 提供 Ollama、LM Studio、OpenAI、DeepSeek、通义兼容接口和自定义 OpenAI-compatible 预设。
- 增加首次启动向导、连接测试、模型配置和密钥管理。
- 保留招聘平台 RPA、本地数据、岗位分析、简历优化、模拟面试、自动打招呼和自动回复。
- 统一本地数据目录，提供打开目录、备份、恢复、清理日志和重置配置能力。
- 自动迁移旧配置和旧数据，不破坏已有本地内容。
- Windows、macOS、Linux 继续作为构建目标。
- 项目采用 Apache License 2.0 发布，仓库根目录包含标准 `LICENSE` 文件。

### 2.2 不包含

- 联网搜索。删除现有 DashScope 搜索请求及配置界面。
- 自动更新。第一版不接 GitHub Releases 或其他更新源。
- SQLite 迁移。继续使用 JSON/YAML。
- Anthropic 等非 OpenAI-compatible 原生协议。
- 插件系统、本地 HTTP 网关、云同步、多用户系统。
- 浏览器 Cookie 的备份和迁移。

## 3. 网络边界

第一版只允许三类外部访问：

1. 用户主动运行招聘自动化时，由受控浏览器访问 Boss 直聘或猎聘。
2. 用户启用 AI 功能时，Rust 后端访问用户配置的 LLM Base URL。
3. 用户主动打开网页链接时，由系统浏览器访问该链接。

禁止：

- 访问 `fk.pgthinker.me` 或其他原项目业务服务。
- 自动更新、遥测、崩溃上报和隐式联网搜索。
- 未经用户配置向第三方模型发送简历、岗位或聊天数据。

源码测试和发布检查必须扫描原服务器域名及旧接口路径，防止回归。

## 4. 总体架构

```text
React UI
  │ Tauri invoke / event
  ▼
Rust Commands
  ├── ConfigService ─────────── app_config.yaml
  ├── CredentialStore ──────── OS Keychain / 环境变量
  ├── LocalStore ───────────── data/*.json
  ├── Recruitment RPA ──────── Boss / 猎聘
  └── LlmService
       └── OpenAiCompatibleProvider ── 用户配置的 Base URL
```

原 `server_api` 不再是架构层。账户功能整体删除，生成能力移动到独立 `llm_provider` 模块。上层业务不得引用服务端 Token、积分、RuoYi 响应或原服务器错误类型。

## 5. 首次启动与主界面

### 5.1 启动流程

应用启动后直接读取本地配置，不检查登录 Token：

```text
启动
  ├── 初始化本地存储与迁移
  ├── 检测 Chrome / Edge
  ├── 已完成引导 → 主界面
  └── 未完成引导 → 初始化向导
```

初始化向导包含：

1. 隐私和网络边界说明。
2. 浏览器自动检测结果。
3. LLM Provider 选择。
4. Base URL、模型和 API Key 配置。
5. 真实短请求连接测试。
6. 完成或跳过 AI 配置。

跳过模型配置后，岗位采集、规则筛选和本地数据浏览仍可使用。AI 按钮进入未配置状态，显示原因和直达配置页的操作，不阻塞整个应用。

### 5.2 导航调整

- 删除登录、注册页面。
- 删除“个人中心”中的资料、密码、积分、记录和充值。
- 将“个人中心”替换为“关于与数据”，展示版本、Apache-2.0 许可证及链接、隐私说明和数据管理。
- 配置中心分为：大模型、招聘平台与浏览器、简历、岗位筛选、打招呼与自动回复、数据管理。
- 删除联网搜索配置组。

## 6. LLM 配置与凭据

### 6.1 非敏感配置

`app_config.yaml` 增加版本和引导状态：

```yaml
schema_version: 1
onboarding_completed: false

llm_config: null
```

配置完成后，`llm_config` 才写为对象：

```yaml
llm_config:
  provider: ollama
  base_url: http://127.0.0.1:11434/v1
  model: qwen3
  timeout_seconds: 120
```

字段约束：

- `llm_config: null` 是唯一的“AI 未配置”持久化状态；TypeScript 对应 `LlmConfig | null`，Rust 对应 `Option<LlmConfig>`。
- `onboarding_completed` 与模型配置相互独立。用户跳过 AI 配置时保存 `onboarding_completed: true` 和 `llm_config: null`。
- `provider` 仅决定 UI 预设，所有 Provider 均使用同一个 OpenAI-compatible 实现。
- `base_url` 在 `llm_config` 对象内必填；保存时去除尾部 `/`，请求时拼接 `/chat/completions`。
- `model` 在 `llm_config` 对象内必填。
- `timeout_seconds` 默认 120，允许 10–600。
- API Key 不序列化进 YAML，也不包含在配置导出中。

预设的第一版确定值如下；模型名由用户选择或输入，避免内置很快过时的模型列表。应用可尝试读取 `/models`，但读取失败不阻止手工配置。

| 预设 | Base URL | 默认需要密钥 |
|---|---|---|
| Ollama | `http://127.0.0.1:11434/v1` | 否 |
| LM Studio | `http://127.0.0.1:1234/v1` | 否 |
| OpenAI | `https://api.openai.com/v1` | 是 |
| DeepSeek | `https://api.deepseek.com` | 是 |
| 通义兼容接口 | `https://dashscope.aliyuncs.com/compatible-mode/v1` | 是 |
| 自定义 | 空，由用户输入 | 由服务决定 |

### 6.2 密钥读取优先级

1. 系统凭据库中的 `llm_api_key`。
2. 环境变量 `FUCKJOB_LLM_API_KEY`。
3. 无密钥请求，适用于 Ollama、LM Studio 或用户自建服务。

前端通过独立命令读取“是否已配置密钥”，永远不能读回密钥明文。提供设置、覆盖和清除密钥命令。Linux 凭据库不可用时给出可执行的环境变量方案；不使用不可解释的自制本地加密。

凭据状态命令同时返回有效来源：`keychain`、`environment` 或 `none`。由于 keychain 优先于环境变量：

- “清除模型密钥”只删除 keychain 内容，并在确认文案中明确这一点。
- 如果 `FUCKJOB_LLM_API_KEY` 仍存在，清除后状态立即显示“正在使用环境变量”，不能显示“无密钥”。
- 应用不能修改父进程环境变量；界面提供变量名和移除说明。
- 连接测试和实际生成都使用状态中展示的同一有效来源。

### 6.3 连接测试

连接测试发送一次极短的非流式生成，而不是仅依赖 `/models`，并返回：

- 是否成功。
- 请求耗时。
- 服务返回的模型名（如果有）。
- 明确的失败分类：不可达、鉴权失败、模型不存在、限流、响应不兼容、超时。

另提供可选的流式测试，用于在保存前验证模拟面试所需的 SSE 能力。

## 7. LLM Provider 设计

### 7.1 领域接口

上层使用本地标准类型：

```rust
struct LlmMessage {
    role: LlmRole,
    content: String,
}

struct LlmRequest {
    messages: Vec<LlmMessage>,
}

struct LlmResponse {
    content: String,
    model: Option<String>,
    finish_reason: Option<String>,
    usage: Option<LlmUsage>,
}
```

`LlmService` 暴露：

- `complete(request) -> LlmResponse`
- `stream(request, on_delta) -> LlmResponse`
- `test_connection() -> ConnectionReport`

第一版只有 `OpenAiCompatibleProvider` 实现。非流式响应读取 `choices[0].message.content`；流式响应解析 SSE `data:` 帧、`choices[0].delta.content` 和 `[DONE]`。错误响应保留状态码和经过脱敏的响应摘要。

### 7.2 Prompt 本地化

删除服务端 `GenerateBo { promptTemplate, params }` 协议。所有模板在本地展开，再以标准 `messages` 发送。

- 内置 Prompt 构建函数直接生成用户消息。
- 用户自定义打招呼和回复 Prompt 保持现有 `{{variable}}` 语法。
- 本地模板渲染器替换已知变量，并在运行前报告缺失变量。
- 未知变量不静默替换为空字符串，避免生成错误内容后自动发送。

现有变量保持兼容：

- `job_content`
- `job_description`
- `chat_history`
- `message_content`
- `resume`
- `resume_context`
- `background_context`

### 7.3 业务调用迁移

以下调用全部改为 `LlmService`：

- `src-tauri/src/llm.rs`：自动打招呼、自动回复。
- `command/job.rs`：岗位分析。
- `command/llm.rs`：调试、简历问题预测、简历章节优化。
- `command/mock_interview.rs`：问题和总结流式生成。

返回值删除积分字段。UI 不再展示或依赖消费积分。

## 8. 联网搜索移除

- 删除 `src-tauri/src/search.rs` 及 `AppRuntimeConfig.search_config`。
- 岗位分析不再构造 `web_search_context`，也不调用 DashScope。
- 删除配置中心的“联网搜索”入口及相关 TypeScript 类型。
- `InterviewJobAnalysis.search_summary` 和 `search_sources` 暂时保留并带默认值，只用于读取旧数据；新分析写入空值，避免破坏已有 JSON。
- 后续若恢复联网搜索，必须作为独立规格和独立 Provider 实现。

## 9. 本地数据与迁移

### 9.1 目标布局

```text
app_config_dir/
└── app_config.yaml

app_data_dir/
├── data/
│   ├── job_details.json
│   ├── chat_messages.json
│   ├── interview_analyses.json
│   └── user_resumes.json
├── browser-profile/
├── backups/
└── logs/
```

用户简历从当前 `app_data_dir/user_resumes.json` 迁移到 `data/user_resumes.json`。现有 DAO 文件名不变。浏览器 profile 只调整新安装的默认路径；已有明确路径继续使用，避免丢失登录态。

### 9.2 安全写入与迁移

- 配置和 JSON 存储改为“同目录临时文件写入 + flush + rename”原子替换。
- 迁移前复制带时间戳的备份。
- 迁移成功后才更新 `schema_version`。
- 无法解析的文件不覆盖、不删除；错误中显示具体路径。
- 旧配置缺少 `llm_config`，或其中 `base_url`、`model` 为空时，迁移为 `llm_config: null`；非空旧配置迁移为完整对象。
- 若检测到旧配置明文 `llm_config.api_key`，尝试写入凭据库，成功后从 YAML 移除；失败则保留原文件并要求用户处理。

文件迁移必须可重复执行，并采用以下优先级：

- 只有旧文件存在：验证后复制到目标临时文件，原子落盘；旧文件保留在迁移备份中。
- 只有目标文件存在：验证目标文件并直接使用，不从备份反向覆盖。
- 两者内容相同：使用目标文件，不重复写入。
- 两者都有效但内容不同：对 `user_resumes.json` 按简历名称合并，目标文件中的同名项优先；生成冲突报告并保留两份原始备份。
- 目标损坏但旧文件有效：备份两者，使用旧文件恢复目标并报告恢复行为。
- 两者都损坏：停止迁移且不写入任何一方，提示用户打开目录。

浏览器 profile 不做搬迁：

- 配置中已有明确 `user_data_dir` 时始终沿用。
- 配置为空但旧默认目录 `app_data_dir/default` 存在时，将配置补为该旧目录。
- 只有全新安装且不存在旧默认目录时，才使用 `app_data_dir/browser-profile`。

`schema_version` 防止已完成步骤重复运行；即使用户手工回退版本，上述存在性、内容校验和备份规则也保证迁移幂等。

### 9.3 数据管理

“关于与数据”提供：

- 打开数据目录。
- 导出完整备份包。
- 从备份包恢复，恢复前自动备份当前数据。
- 清理日志。
- 清除模型密钥。
- 重置应用配置。

备份格式固定为 ZIP，包含：

```text
manifest.json
config/app_config.yaml
data/job_details.json
data/chat_messages.json
data/interview_analyses.json
data/user_resumes.json
```

`manifest.json` 包含备份格式版本、应用版本、创建时间和各文件 SHA-256。配置文件包含正常非敏感配置，但不包含 API Key。备份不包含日志、浏览器 profile、Cookie 或系统凭据。

恢复语义固定为“整包替换”，不进行隐式合并：

1. 解压到临时目录，拒绝绝对路径和 `..` 路径。
2. 验证 manifest、校验和、YAML/JSON 结构和支持的 schema 版本。
3. 在任何覆盖前，为当前配置和数据创建完整恢复点。
4. 暂停写入，逐文件使用同目录临时文件和 rename 替换。
5. 任一步骤失败时从恢复点回滚所有已替换文件，并报告失败文件。
6. 全部成功后提示用户重启应用，使内存状态与磁盘完全一致。

高于当前支持版本的备份拒绝恢复；较旧版本先在临时目录运行迁移并验证成功后再替换。恢复界面必须明确列出将被替换的数据并二次确认。

## 10. 错误处理与安全行为

统一 `AppError` 分类：

- `Configuration`
- `Credential`
- `Network`
- `Provider`
- `Storage`
- `Browser`
- `Validation`
- `Cancelled`

Tauri 命令继续使用统一结果壳，但错误增加稳定的 `code`、用户可读 `message` 和可选 `detail`。界面按 code 给出对应操作，例如“打开模型配置”“重新登录招聘平台”“打开数据目录”。

安全规则：

- 日志不记录 API Key、Authorization、Cookie、完整简历或完整聊天正文。
- 自动生成失败时不得发送空字符串、错误文本或半段流式内容。
- 自动回复只有完整生成成功后才允许发送；若配置了普通模板，可按现有优先级明确回退。
- 连接超时不无限重试。只对尚未产生副作用的模型请求进行有限重试，招聘平台发送动作不自动重试。
- 用户取消长任务时返回 `Cancelled`，不显示为系统故障。

## 11. 删除与清理

删除或停用：

- `src-tauri/src/server_api/auth.rs`
- `src-tauri/src/server_api/base.rs` 中原服务器协议、加密和 Token 逻辑
- `src-tauri/src/server_api/config.rs`
- `src-tauri/src/server_api/generate.rs`
- `src-tauri/src/command/auth.rs`
- `src/lib/tauriAuth.ts`
- `src/view/auth/`
- 原账户页中的账户、积分和充值实现
- `src/lib/updater.tsx`
- `tauri-plugin-updater`、`tauri-plugin-process` 及对应 capability
- `tauri.conf.json` 的 updater endpoint 和更新产物配置
- `src-tauri/src/search.rs`
- 不再使用的 RSA/AES/ECB/加密、Client ID 和 RuoYi 依赖代码

保留 `reqwest` 作为 LLM Provider HTTP 客户端。删除依赖前必须通过引用扫描确认无其他用途。

## 12. 测试策略

### 12.1 Rust 单元测试

- Base URL 规范化和 endpoint 拼接。
- OpenAI-compatible 非流式成功、错误和畸形响应解析。
- SSE 分片、跨 chunk 帧、多个 delta、`[DONE]`、错误帧解析。
- Prompt 变量替换、缺失变量、未知变量。
- 凭据读取优先级和日志脱敏。
- keychain、环境变量、无密钥之间的有效凭据来源回退，以及清除 keychain 后的状态变化。
- 配置默认值、旧配置迁移、迁移失败保护。
- JSON 原子写入和损坏文件保护。
- 备份校验、拒绝路径穿越、整包恢复成功和中途失败后的完整回滚。
- 旧分析数据中搜索字段的反序列化兼容。

HTTP 测试使用本地 mock server，不访问真实厂商。

### 12.2 前端测试与构建检查

- 未完成引导、跳过模型、配置成功三种启动状态。
- Provider 预设切换不覆盖用户已明确编辑的值。
- 密钥只可写入或清除，不能从后端读回。
- AI 未配置时按钮状态和跳转行为。
- 删除登录、账户、积分、充值、搜索和更新入口。
- TypeScript 构建通过。

### 12.3 集成与人工冒烟

至少验证：

- Ollama 无密钥普通生成和流式生成。
- 一个带 API Key 的 OpenAI-compatible 服务。
- Boss 和猎聘登录检查及至少一个无发送副作用的采集流程。
- 岗位分析、简历优化、模拟面试和沟通调试。
- 备份、恢复和旧数据迁移。
- macOS、Windows、Linux 打包构建。

## 13. 第一版验收标准

满足以下条件才可发布：

1. 全新安装无需账号即可进入并使用非 AI 功能。
2. 用户可在 UI 中完成模型配置、密钥保存和连接测试。
3. 普通生成与流式生成均不经过原项目服务器。
4. 所有现有 AI 功能都通过统一 LLM Provider 工作。
5. 应用运行期间不会请求原项目域名、更新服务或 DashScope 搜索接口。
6. 招聘平台自动化仍能使用已有浏览器登录态。
7. 旧配置和本地数据可迁移，失败时不丢数据。
8. API Key 不出现在配置导出、日志或前端读取结果中。
9. 自动回复生成失败时不会误发内容。
10. Rust 测试、前端构建和目标平台打包检查通过。

## 14. 实施分段

这是一个统一目标下的分阶段改造：

1. 建立配置版本、凭据存储和 LLM Provider，并用测试锁定协议。
2. 迁移所有 AI 调用，删除原生成网关和联网搜索。
3. 删除鉴权、账户、积分和更新体系，重构启动流程。
4. 实现初始化向导、模型配置和关于与数据页面。
5. 加固本地存储、迁移、备份和错误处理。
6. 完成网络边界检查、回归测试、跨平台打包和开源文档。

开源文档至少包括根目录 `LICENSE`（Apache License 2.0）、面向用户的 README、隐私与网络边界说明、模型配置示例和本地开发/构建步骤。发布物的“关于与数据”页面必须能查看许可证名称并打开仓库中的完整许可文本。

每一阶段都应保持可编译，并在删除旧模块前完成调用迁移。
