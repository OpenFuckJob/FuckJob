# GitHub 自动发版与更新

客户端只使用公开的 GitHub Releases：

```text
https://github.com/OpenFuckJob/FuckJob/releases/latest/download/latest.json
```

普通 `master` 推送只运行 CI。只有 `v*` 标签会触发 `.github/workflows/release.yml`，依次完成版本校验、测试、多平台构建、更新包签名、Draft Release 聚合以及正式发布。

## 首次配置

在仓库 `Settings → Secrets and variables → Actions` 中创建：

- `TAURI_SIGNING_PRIVATE_KEY`：本地 `.tauri/offerflow.key` 的完整内容。
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：当前密钥无密码时可以不创建；以后换成带密码密钥时再设置。

私钥必须单独备份，不能提交到 Git 或上传为 Release Asset。客户端内置的公钥和后续发布使用的私钥必须保持配对。

## 发布版本

先同步修改以下三个版本号，例如都改成 `0.1.3`：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

提交并推送代码，确认普通 CI 通过后创建标签：

```bash
git tag v0.1.3
git push origin v0.1.3
```

标签版本必须与三个文件完全一致，否则 Release 工作流会在构建前失败。

## 工作流产物

工作流构建：

- Windows x86_64：NSIS 安装包和对应 `.sig`。
- macOS Apple Silicon：DMG、`.app.tar.gz` 和对应 `.sig`。
- macOS Intel：DMG、`.app.tar.gz` 和对应 `.sig`。

`tauri-apps/tauri-action` 会自动将安装包、更新包、签名和 `latest.json` 上传到同一个 Draft Release。只有全部平台成功后，最后一个 job 才把 Release 发布并标记为 Latest；任一平台失败时 Draft 不会暴露给客户端。

发布完成后检查 Release Assets 至少包含：

```text
latest.json
*.exe
*.exe.sig
*.dmg
*.app.tar.gz
*.app.tar.gz.sig
```

不再需要手写或上传 `update.json`，也不需要维护外部更新服务器。
