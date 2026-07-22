# Fuck Job（本地优先版）

Fuck Job 是一个基于 Tauri 2、React 和 Rust 的桌面求职工具。它把配置、岗位记录、沟通记录、分析结果和简历数据保存在本机，并通过本机 Chrome/Edge 自动化辅助 BOSS 直聘和猎聘的职位筛选、主动沟通、未读回复与周期任务。

本地优先版不需要 Fuck Job 账号，不连接原项目业务服务器，不包含遥测、联网搜索或自动更新器。AI 完全可选；不配置模型也可以使用本地配置、招聘平台自动化、岗位数据和备份功能。

## 功能与平台

- BOSS 直聘、猎聘的浏览器环境检查、扫码登录、单轮/周期求职和未读回复。
- 本地岗位、沟通记录和运行日志；可抓取已沟通过岗位并按平台查看。
- 可选的岗位分析、打招呼/回复生成和沟通调试，使用 OpenAI 兼容接口。
- 六种模型预设：Ollama、LM Studio、OpenAI、DeepSeek、DashScope 和自定义端点。
- 配置导入/导出、完整数据备份/恢复、日志清理和模型密钥管理。
- Tauri 打包目标包含 macOS（`.app`/`.dmg`）、Windows（NSIS）和 Linux（`.deb`/`.AppImage`）。发布前仍应在各目标系统原生验证；跨平台 Rust 编译不等于安装包验证。

当前限制：自动化依赖招聘网站 DOM，网站改版、风控或登录流程变化可能导致任务失败；猎聘暂不发送图片话术；需要本机安装 Chrome 或 Edge（也可手动指定 Chromium 浏览器路径）；没有联网搜索、自动更新器和内置云端同步。

## 网络边界与隐私

应用只有以下出站边界：

1. 仅在你检查招聘环境、抓取岗位或运行自动化时访问 BOSS 直聘或猎聘。
2. 仅在你获取模型列表、测试连接或使用 AI 功能时访问所配置的 LLM 地址。
3. 仅在你点击仓库、许可、文档或内容中的外部链接时，由系统浏览器打开相应地址。

除此之外，运行时代码不连接原项目服务器，不发送遥测，不检查更新，也不执行 Web 搜索。详细说明与审计方法见 [隐私与网络边界](docs/privacy-and-network.md)。

> 使用 OpenAI、DeepSeek、DashScope 或其他远程端点时，岗位描述、简历、聊天上下文或提示词可能发送给该服务商。请先阅读服务商隐私条款，不要提交不希望离开设备的内容。若需要数据留在本机，可使用 [Ollama 或 LM Studio](docs/model-configuration.md)。

## 首次启动

首次启动包含三步：

1. 阅读本地存储和可选网络请求说明；可以立即“跳过 AI，进入应用”。
2. 检测 Chrome/Edge 与独立浏览器数据目录。未检测到时，可稍后在“配置中心”手动选择浏览器程序和数据目录。
3. 可选配置 LLM：选择预设、填写模型名称，必要时保存 API Key，并运行短连接或流式测试。AI 配置失败不会阻止本地功能。

进入应用后，先在“配置中心”确认招聘筛选、话术、浏览器和简历上下文，再到“工作台”选择平台和任务类型。招聘平台登录使用对应 App 扫码，登录态保存在应用自己的浏览器配置目录中。

## 模型配置速览

预设地址如下：

| 预设 | 默认地址 | 默认要求 API Key |
| --- | --- | --- |
| Ollama | `http://127.0.0.1:11434/v1` | 否 |
| LM Studio | `http://127.0.0.1:1234/v1` | 否 |
| OpenAI | `https://api.openai.com/v1` | 是 |
| DeepSeek | `https://api.deepseek.com` | 是 |
| DashScope | `https://dashscope.aliyuncs.com/compatible-mode/v1` | 是 |
| 自定义 | 手动填写 | 取决于端点 |

配置至少需要服务地址、模型名称和 10–600 秒的超时。可以从服务读取模型列表；若端点不支持 `/models`，直接手动输入模型 ID。API Key 优先从系统钥匙串/凭据库读取，未找到时回退到 `FUCKJOB_LLM_API_KEY` 环境变量。完整步骤和错误排查见 [模型配置](docs/model-configuration.md)。

## 本地数据、备份与恢复

应用通过 Tauri 的平台目录 API 定位文件，不依赖文档中写死的 OS 路径：

- `app_config_dir/app_config.yaml`：应用配置。
- `app_data_dir/data/`：岗位、聊天、分析和用户简历 JSON。
- `app_data_dir/browser-profile/`：默认的独立浏览器资料目录。
- `app_data_dir/rpa.log`：本地运行日志。
- `app_data_dir/backups/`：迁移前备份；`app_data_dir/recovery/`：恢复前自动生成的恢复点。
- 模型 API Key：系统钥匙串/凭据库，不写入上述配置和数据文件。

实际解析后的 `app_data_dir` 可在“关于与数据”中查看并在文件管理器中打开；`app_config_dir` 和 `app_data_dir` 的 OS 映射由 Tauri 决定。

“导出备份”生成带清单和 SHA-256 校验的 ZIP，只包含应用配置及岗位、聊天、分析、用户简历数据。它不包含 API Key、日志、浏览器登录态/资料目录、迁移备份或既有恢复点；配置中的敏感字段会再次清理。

恢复会先完整校验版本、允许文件列表、路径和校验和，再创建恢复前备份并替换数据。中途任何写入失败都会尝试回滚所有目标文件；成功后需要手动重启应用。备份仍可能含简历、岗位和聊天等私人内容，请自行加密保管。

## 本地开发

前置条件：

- Node.js 20.19+ 和 pnpm（可通过 Corepack 启用）。
- Rust stable toolchain。
- 对应系统的 [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/)；运行招聘自动化还需要 Chrome 或 Edge。

```bash
corepack enable
pnpm install --frozen-lockfile

# 仅启动 Vite 前端
pnpm dev

# 启动 Tauri 桌面开发环境
pnpm tauri dev

# 前端类型检查与生产构建
pnpm build

# Rust 格式、测试和静态检查
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

## 打包

请在目标操作系统原生构建安装包：

```bash
# macOS
pnpm tauri build --bundles app,dmg

# Windows（在 Windows 上）
pnpm tauri build --bundles nsis

# Linux（在 Linux 上，需安装发行版对应的 Tauri/WebKitGTK 打包依赖）
pnpm tauri build --bundles deb,appimage
```

也可使用仓库脚本：

```bash
./build.sh            # 当前平台原生安装包
./build.sh macos      # macOS .app/.dmg
./build.sh linux      # Linux 原生包；非 Linux 主机仅 cross 编译 Rust
./build.sh windows    # Windows 原生 NSIS；非 Windows 主机仅 cross 编译 Rust
./build.sh all        # 当前仅在 macOS 构建 macOS 包
```

脚本会执行锁定依赖安装和前端/Tauri 构建，并把便于分发的副本收集到 `releases/<version>/<platform>/`；Tauri 原始产物仍在 `src-tauri/target/.../release/bundle/`。非原生 `linux`/`windows` 模式依赖 Docker 与 `cross`，只验证 Rust 交叉编译，不生成或验证可发布安装包。

## 测试与网络审计

```bash
pnpm test:run
pnpm build
pnpm check:network
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# 补充人工扫描（排除依赖和构建目录）
rg -n --glob '!node_modules/**' --glob '!src-tauri/target/**' \
  'fk\.pgthinker\.me|/prod-api|tauri[-_]plugin[-_]updater|update\.json|enable_search' \
  src src-tauri scripts package.json build.sh
```

`pnpm check:network` 扫描运行时代码、配置和构建脚本，禁止原服务器、旧账号/积分接口、旧 RPA 生成接口、硬编码搜索开关和更新器引用。

## 贡献与安全

欢迎提交可复现的问题和范围清晰的 Pull Request。涉及招聘网站适配时，请避免提交真实 Cookie、聊天、简历、API Key 或个人浏览器资料。发现可能泄露凭据、绕过网络边界或破坏备份恢复的问题，请不要在公开 issue 中附带敏感样本；先通过仓库维护者可用的私密联系方式报告，并用脱敏数据复现。

项目采用 [Apache License 2.0](LICENSE)。使用自动化功能时，请遵守招聘平台条款、当地法律及合理的请求频率；本项目不保证第三方网站兼容性。
