# 模型配置

Fuck Job 使用 Rig 按所选 provider 调用大模型服务。AI 是可选能力；清空模型配置不会影响本地数据和招聘平台非 AI 功能。

## 预设

| 预设 | 默认 Base URL | API Key |
| --- | --- | --- |
| Anthropic | `https://api.anthropic.com` | 需要 |
| DeepSeek | `https://api.deepseek.com` | 需要 |
| OpenAI (Completions) | `https://api.openai.com/v1` | 需要 |
| OpenAI (Responses) | `https://api.openai.com/v1` | 需要 |
| MiniMax | `https://api.minimax.io/v1` | 需要 |
| Moonshot | `https://api.moonshot.ai/v1` | 需要 |
| Ollama | `http://127.0.0.1:11434` | 默认不需要 |
| OpenRouter | `https://openrouter.ai/api/v1` | 需要 |
| Xiaomi MiMo | `https://api.xiaomimimo.com/v1` | 需要 |
| Z.ai | `https://api.z.ai/api/paas/v4` | 需要 |

选择预设会填入对应 provider 的默认 Base URL 并清空模型名称。服务地址输入框始终可编辑；如需使用官方区域端点、私有网关或兼容代理，可在选择预设后直接修改 Base URL。服务地址保存时会去掉末尾 `/`。

## Ollama（本地）

1. 安装并启动 Ollama。
2. 在终端拉取支持对话的模型，例如 `ollama pull qwen2.5:7b`。
3. 在“配置中心 → 大模型配置”选择 Ollama。
4. 保持 `http://127.0.0.1:11434`，点击“获取模型列表”或手动输入已拉取的模型名。
5. 运行“短连接测试”，确认配置可用后保存。

Ollama 现在使用 Rig 原生 provider。历史配置中的 `http://127.0.0.1:11434/v1` 会在运行时兼容为根地址，但新配置建议直接使用 `http://127.0.0.1:11434`。

本机 `127.0.0.1`/`localhost` 地址会绕过系统代理。若 Ollama 运行在另一台机器，请直接修改服务地址；此时数据会离开本机，并应自行配置 TLS、鉴权和访问控制。

## 在线 provider 与自定义 BaseURL

配置字段含义：

- 服务预设：决定后端使用哪个 Rig provider，以及界面对密钥的必填检查。
- 服务地址：provider 的 API 根地址。默认值来自 provider 的官方默认端点；用户可按需改为区域端点、私有网关或兼容代理地址。
- 模型：请求体中的精确模型 ID，区分由服务商定义。
- API Key：保存到系统钥匙串/凭据库，不写入 YAML，也不会在界面回显。

除 Ollama 外，Anthropic、DeepSeek、OpenAI (Completions)、OpenAI (Responses)、MiniMax、Moonshot、OpenRouter、Xiaomi MiMo 与 Z.ai 都要求先保存 API Key。当前只有一个全局模型密钥，切换服务商时应替换它。

OpenAI (Completions) 与 OpenAI (Responses) 默认都使用 `https://api.openai.com/v1`，但后端调用路径不同：OpenAI (Completions) 发送到 `<base_url>/chat/completions`，OpenAI (Responses) 发送到 `<base_url>/responses`。如需连接 OpenAI-compatible 网关，通常选择 OpenAI (Completions)。

## 模型列表与手动模型

“获取模型列表”会按 provider 使用对应的 Rig 模型列表能力。Anthropic、DeepSeek、OpenAI (Completions)、OpenAI (Responses)、Ollama、OpenRouter、Xiaomi MiMo 支持从服务端拉取模型列表；MiniMax、Moonshot、Z.ai 当前在 Rig 中未暴露模型列表能力，请手动填写模型 ID。

OpenAI (Completions) 与 OpenAI (Responses) 获取模型列表时共用 OpenAI 模型列表接口，通常访问 `<base_url>/models`。

模型列表成功后，模型会进入输入框建议，但仍需选择或输入名称。部分代理、服务商或本地服务器没有实现列表端点；列表失败不代表聊天接口必然失败，此时直接填写文档中的模型 ID 并测试连接。

## 连接测试

“短连接测试”会通过当前 provider 发送固定提示并等待普通响应，可验证地址、Key、模型和基本生成能力。该测试是实际模型请求，远程服务可能计费。测试成功只证明当前配置可响应，不保证所有长提示或招聘自动化场景都不会超时。

## 环境变量回退

无钥匙串条目时，可在启动应用的环境中设置：

```bash
export FUCKJOB_LLM_API_KEY='your-key'
pnpm tauri dev
```

解析优先级是系统钥匙串/凭据库，其次环境变量。若界面显示来源为 `keychain`，环境变量不会覆盖它；先在模型配置中清除已保存密钥。清除按钮不会删除 shell、launchd、systemd 或其他启动器中的环境变量。

## 错误排查

界面错误通常带结构化类别：

### `configuration` / `validation`

- Base URL 或模型为空：填写完整字段并保存。
- URL 拼接错误：Base URL 应是所选 provider 的 API 根地址，不要填到具体的聊天接口路径。

### `credential`

- 401/403：确认 Key 属于当前服务、未过期且有目标模型权限。
- 切换服务商后仍失败：全局钥匙串 Key 可能仍是上一服务商的，替换或清除后让环境变量生效。
- 本地服务意外要求 Key：检查本地网关设置，或保存其 Bearer Key。

### `network`

- 无法连接：确认服务进程、主机、端口、防火墙、DNS 和 TLS 证书。
- 请求超时：先用更小模型测试，确认模型已加载，再适当增加超时。
- 本地地址受代理影响：应用对 `127.0.0.1` 和 `localhost` 主动禁用代理；远程/局域网地址仍遵循 HTTP 客户端环境。
- 流式意外中断：检查反向代理是否缓冲/截断 SSE，并确认服务发送结束标记。

### `provider`

- 404：Base URL 层级或模型 ID 不正确。
- 429：服务限流或额度不足，降低频率并检查账户配额。
- 模型列表不可用或解析失败：手动输入模型 ID，再测试聊天接口。
- 响应缺少候选或文本：端点返回格式与当前 Rig provider 预期不兼容。

### AI 生成未发送

自动化只发送非空的显式话术或成功生成的非空文本。模型失败或返回空白时，LLM 占位内容会被跳过；若同一模板还配置了明确文本，则只发送这些明确文本。查看本地日志中的脱敏错误，并先在“沟通调试”复现。

更多数据发送范围见 [隐私与网络边界](privacy-and-network.md)。
