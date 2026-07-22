# Fuck Job 🚀

> 本地优先的开源桌面求职自动化与分析工具

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](https://github.com/OpenFuckJob/FuckJob/blob/master/LICENSE)

**Fuck Job** 是一款基于 [Tauri 2](https://tauri.app/) 构建的跨平台桌面应用，专注于求职流程的自动化与智能化。它完全本地运行，无需账号、无遥测、无云端同步，你的数据始终掌握在自己手中。

---

## ✨ 功能概览

### 🖥️ 工作台
一键自动化求职流程——环境检测、扫码登录、岗位抓取、批量沟通，支持 **BOSS 直聘**和**猎聘**两大平台。

- 自动化浏览器操控，模拟真实用户行为
- 支持单轮/周期求职模式
- 实时日志查看与流程监控
- 可配置的打招呼话术与回复模板

### 📋 岗位管理
集中管理抓取的岗位数据，支持筛选、搜索和 AI 辅助分析。

- 岗位详情浏览与管理
- 薪酬、经验、学历、行业等多维度筛选
- AI 岗位分析与匹配度评估

### 🤖 AI 能力（可选）
兼容 OpenAI 接口的大模型集成，支持：

- **本地模型**：Ollama、LM Studio（数据不出本机）
- **在线服务**：OpenAI、DeepSeek、阿里云 DashScope
- **自定义端点**：任意 OpenAI 兼容 API

AI 功能包括：
- 📝 简历优化与模拟面试
- 💬 智能打招呼语生成
- 📊 岗位描述分析与匹配
- 🔄 聊天回复自动生成

### ⚙️ 配置中心
灵活可调的求职策略配置：

- **大模型配置**：服务地址、模型选择、API Key（存储于系统钥匙串）
- **简历配置**：多份简历管理与上下文注入
- **岗位筛选**：正则规则过滤，支持 ACCEPT/REJECT 模式
- **沟通话术**：可配置的打招呼模板与回复规则
- **浏览器配置**：Chromium 路径、启动参数等

### 🔒 关于与数据
透明可控的数据管理：

- 应用版本与数据目录查看
- 数据导出备份 / 恢复备份
- 日志清除与模型密钥管理
- 应用配置重置
- 查看 [完整许可](https://github.com/OpenFuckJob/FuckJob/blob/master/LICENSE)、[隐私与网络文档](https://github.com/OpenFuckJob/FuckJob/blob/master/docs/privacy-and-network.md)、[模型配置文档](https://github.com/OpenFuckJob/FuckJob/blob/master/docs/model-configuration.md)

---

## 🛡️ 隐私优先

- ✅ **完全本地运行**：配置、岗位、聊天、简历数据仅存储在本机
- ✅ **无遥测**：不含任何数据收集或上报
- ✅ **无后台服务器**：不依赖原项目业务服务器，无账号体系
- ✅ **凭据安全**：API Key 存储在系统钥匙串/凭据库，不写入配置文件
- ✅ **日志脱敏**：敏感字段（密钥、Token、Cookie 等）自动脱敏
- ✅ **网络边界可审计**：运行 `pnpm check:network` 验证出站请求

详见 [隐私与网络边界文档](https://github.com/OpenFuckJob/FuckJob/blob/master/docs/privacy-and-network.md)。

---

## 🛠️ 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | [Tauri 2](https://tauri.app/) |
| 前端 | React 19 + TypeScript |
| UI 组件 | Ant Design 6 + Tailwind CSS 4 |
| 构建工具 | Vite 7 |
| 后端 | Rust (reqwest, rig-core, rust_drission 等) |
| 测试 | Vitest + Testing Library |
| 包管理 | pnpm |

---

## 📦 快速开始

### 环境要求

- [Rust](https://www.rust-lang.org/) (stable)
- [Node.js](https://nodejs.org/) ≥ 18
- [pnpm](https://pnpm.io/) ≥ 9
- 系统依赖：参考 [Tauri 2 前置要求](https://tauri.app/start/prerequisites/)

### 开发运行

```bash
# 安装依赖
pnpm install

# 启动开发模式
pnpm tauri dev
```

### 构建发布

```bash
pnpm tauri build
```

### macOS 使用说明

由于本项目未经过 Apple 官方签名公证，从 [Releases](https://github.com/OpenFuckJob/FuckJob/releases) 下载 `.dmg` 安装后，macOS Gatekeeper 会阻止应用打开。请先运行以下命令移除隔离标记：

```bash
xattr -dr com.apple.quarantine /Applications/fuckJob.app
```

> **提示**：如果你通过 `pnpm tauri build` 自行构建，生成的 `.app` 同样需要执行此命令才能正常运行。

### 运行测试

```bash
pnpm test:run
```

### 网络边界检查

```bash
pnpm check:network
```

---

## 📁 项目结构

```
FuckJob/
├── src/                    # React 前端
│   ├── components/         # 通用组件
│   ├── hooks/              # 自定义 Hooks
│   ├── lib/                # 工具函数与常量
│   ├── types/              # TypeScript 类型定义
│   ├── view/               # 页面视图
│   │   ├── workspace/      # 工作台
│   │   ├── job-data/       # 岗位管理
│   │   ├── config/         # 配置中心
│   │   ├── about-data/     # 关于与数据
│   │   ├── resume-optimizer/ # 简历优化
│   │   ├── conversation-debug/ # 沟通调试
│   │   └── onboarding/     # 初次引导
│   └── assets/             # 静态资源
├── src-tauri/              # Rust 后端
│   └── src/
│       ├── rpa/            # 浏览器自动化
│       ├── llm/            # 大模型集成
│       ├── storage/        # 数据持久化
│       └── dao/            # 数据访问层
├── docs/                   # 文档
│   ├── model-configuration.md
│   └── privacy-and-network.md
└── scripts/                # 工具脚本
```

---

## 📄 许可

本项目基于 [Apache License 2.0](https://github.com/OpenFuckJob/FuckJob/blob/master/LICENSE) 开源。

---

## ⚠️ 免责声明

本工具仅供个人求职辅助用途。使用者应遵守各招聘平台的服务条款，合理使用自动化功能。开发者不对因使用本工具导致的任何账号限制、数据丢失或其他后果承担责任。
