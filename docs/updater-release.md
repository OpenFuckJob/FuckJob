# GitHub 自动发版与更新

项目的版本发布、安装包托管和客户端自动更新全部使用公开的 GitHub Releases，不依赖外部更新服务器。客户端固定请求：

```text
https://github.com/OpenFuckJob/FuckJob/releases/latest/download/latest.json
```

普通 `master` 推送只运行 CI。只有推送 `v*` 标签才会触发 `.github/workflows/release.yml`，自动完成版本校验、测试、多平台构建、更新包签名、Draft Release 聚合和正式发布。

## 完整流程

```text
统一修改应用版本号
        ↓
提交并推送 master
        ↓
确认普通 CI 通过
        ↓
创建并推送 v<版本号> 标签
        ↓
校验标签与应用版本
        ↓
运行前端测试、Cargo Check 和 Rust 测试
        ↓
创建 Draft Release
        ↓
并行构建 Windows、macOS Intel、macOS Apple Silicon
        ↓
生成更新包、数字签名和 latest.json
        ↓
全部平台成功后公开 Release 并标记为 Latest
        ↓
客户端启动时检测、下载、验签、安装并重启
```

## 首次配置

在仓库 `Settings → Secrets and variables → Actions` 中创建：

- `TAURI_SIGNING_PRIVATE_KEY`：本地 `.tauri/offerflow.key` 的完整内容。
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：当前密钥无密码时可以不创建；以后换成带密码密钥时再设置。

私钥必须单独备份，不能提交到 Git 或上传为 Release Asset。客户端内置的公钥和后续发布使用的私钥必须保持配对。

公钥配置在 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey` 中。私钥丢失后无法继续为已经安装的客户端发布可信更新；如果更换密钥，旧客户端也无法验证新密钥生成的更新包。

## 发布新版本

以下示例将应用从当前版本更新至 `0.1.4`。

### 1. 统一修改版本号

将以下三个文件中的版本号全部修改为 `0.1.4`，文件中的版本号不带 `v`：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

执行 Rust 检查后，如果 `src-tauri/Cargo.lock` 中的应用版本发生变化，也要将它一起提交。

标签必须使用带 `v` 的相同版本，例如 `v0.1.4`。`scripts/check-release-version.mjs` 会检查标签格式和上述三个版本号；任意一处不一致，Release 工作流都会在打包前失败。

### 2. 提交并推送代码

```bash
git add .
git commit -m "chore: release v0.1.4"
git push origin master
```

建议先确认普通 CI 通过。无需每次在本地执行全量原生打包，完整的跨平台打包由 GitHub Actions 完成。

### 3. 创建并推送发布标签

```bash
git tag -a v0.1.4 -m "Release v0.1.4"
git push origin v0.1.4
```

推送标签后，可以在仓库的 `Actions → Release` 页面查看发版进度。

## GitHub Actions 发版阶段

### 1. `verify`：版本与代码校验

工作流首先执行：

```text
node scripts/check-release-version.mjs "${GITHUB_REF_NAME}"
pnpm install --frozen-lockfile
pnpm test:run
cargo check
cargo test
```

只有版本校验和测试全部成功，才会进入后续发版阶段。

### 2. `create-draft`：创建草稿 Release

工作流创建标题类似 `OfferFlow v0.1.4` 的 Draft Release。发布前可以创建 `docs/releases/v<版本号>.md` 作为该版本的正式描述；存在对应文件时工作流优先使用该文件，否则根据上一个标签自动生成 GitHub Release Notes。

如果同标签的 Draft Release 已经存在，工作流会复用它；如果该标签对应的 Release 已经公开，工作流会停止，避免覆盖已发布版本。

Draft Release 不会被 `/releases/latest/` 返回，因此打包尚未完成的版本不会被客户端检测到。

### 3. `build`：多平台构建与签名

三个构建任务并行执行：

| 平台 | 架构 | 构建产物 |
| --- | --- | --- |
| Windows | x86_64 | NSIS 安装包和更新签名 |
| macOS | Apple Silicon / aarch64 | DMG、`.app.tar.gz` 和更新签名 |
| macOS | Intel / x86_64 | DMG、`.app.tar.gz` 和更新签名 |

`tauri-apps/tauri-action` 使用 GitHub Secrets 中的 Tauri 私钥签名更新包，并将安装包、更新包和 `.sig` 文件上传到 Draft Release。

工作流同时自动生成并上传 `latest.json`。该文件包含最新版本、发布时间、版本描述、不同平台的更新包地址以及对应签名，所以不需要手写或上传 `update.json`。发布流程会将 GitHub Release 正文同步到 `latest.json` 的 `notes` 字段，供客户端更新弹窗展示。

### 4. `publish`：公开完整版本

只有全部平台构建、签名和上传成功后，`publish` 任务才会：

- 将 Draft Release 改为公开 Release。
- 将该 Release 标记为 Latest。
- 让固定的 `releases/latest/download/latest.json` 地址指向新版本。

任一平台失败时，`publish` 不会执行，Release 会继续保持 Draft，不会向客户端暴露不完整版本。

## Release 产物与签名位置

发布完成后，GitHub Release Assets 至少应该包含：

```text
latest.json
*.exe
*.exe.sig
*.dmg
*.app.tar.gz
*.app.tar.gz.sig
```

Windows 用户手动安装使用 `.exe`，macOS 用户手动安装使用对应架构的 `.dmg`。自动更新使用 Windows 的 NSIS 安装包或 macOS 的 `.app.tar.gz` 更新包。

签名同时存在于两个位置：

- Release Assets 中与更新包同名的 `.sig` 文件。
- `latest.json` 对应平台的 `signature` 字段。

客户端实际验签使用 `latest.json` 中的签名和应用内置公钥。`.sig` 文件主要用于发布产物留档和人工核验。签名不等同于 Windows Authenticode 或 Apple Developer ID 代码签名；它用于证明自动更新包由项目持有的 Tauri 私钥生成且内容未被篡改。

## 客户端更新流程

客户端的更新交互实现在 `src/lib/updater.tsx`。

### 1. 启动时检查

应用启动后，`AutoUpdater` 只在 Tauri 桌面端执行一次更新检查：

```ts
check({ timeout: 15_000 })
```

浏览器开发环境不会检查更新。更新服务暂时不可用或网络超时时，只记录警告，不会阻止应用启动。

### 2. 比较版本并提示

Tauri Updater 请求 GitHub 上的 `latest.json`，将其中的版本与本机版本比较。线上版本更高时，应用显示“发现新版本”弹窗；用户可以选择“立即更新”或“稍后更新”。

### 3. 下载、验签和安装

用户点击“立即更新”后：

1. 根据操作系统、CPU 架构和安装器类型选择对应更新包。
2. 从 GitHub Release 下载更新包并显示下载进度。
3. 使用 `src-tauri/tauri.conf.json` 中的公钥验证更新包签名。
4. 只有签名正确时才安装更新。
5. Windows 使用 `passive` 安装模式。
6. 安装完成后自动重启应用。

如果更新包损坏、被替换或不是由配套私钥签名，签名验证会失败，客户端不会安装该更新。

## 发布后检查

发版工作流成功后，至少检查以下内容：

1. GitHub Actions 中的 `verify`、三个 `build` 和 `publish` 任务全部成功。
2. Release 已公开并标记为 Latest，而不是 Draft。
3. Release Assets 包含三个平台的安装包、更新包、`.sig` 和 `latest.json`。
4. 固定地址可以访问，并且 `version` 是刚发布的版本：

   ```text
   https://github.com/OpenFuckJob/FuckJob/releases/latest/download/latest.json
   ```

5. `latest.json` 包含以下平台及非空 `signature`：

   ```text
   windows-x86_64
   darwin-x86_64
   darwin-aarch64
   ```

6. Windows 安装包和两个 macOS DMG 可以从公开 Release 正常下载。

## 当前限制和注意事项

- 当前只构建 Windows x86_64、macOS Intel 和 macOS Apple Silicon，不构建 Linux 安装包。
- 客户端弹窗显示 `latest.json` 的 `notes`。建议每个正式版本维护 `docs/releases/v<版本号>.md`，避免更新弹窗只显示自动生成的提交记录。
- 不要删除、覆盖或移动已经发布的标签。
- 不要公开 `TAURI_SIGNING_PRIVATE_KEY`，也不要把它作为 Release Asset 上传。
- 不要在三个平台构建完成前手动公开 Draft Release。
- 不需要维护外部更新接口，也不需要手写 `update.json`。
