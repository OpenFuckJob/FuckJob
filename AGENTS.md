# FuckJob — Agent 宪法

本项目由自治 agent 循环（loop）维护。人是 nightly 审查员，agent 是执行者。
所有 agent 必须先读本文件，并遵守 `@LESSONS.md` 中的全部教训。

## 项目事实

- Tauri 2 桌面应用：Rust 后端（`src-tauri/`）+ TypeScript/React 前端（`src/`）
- 包管理：pnpm（前端）、cargo（Rust）
- 本工作区是 `alma/agent-loops` worktree，专门给 agent loop 用，**不要动用户主目录 `~/workSpace/FuckJob`**

## 铁律（违反任何一条 = 本次迭代作废）

1. **验证优先**：改完代码必须跑 `pnpm build`（前端）和 `cargo check`（src-tauri/），全绿才算完成。跑不过就回滚自己的改动，不要留下编译不动的代码。
2. **最小 diff**：只改与当前任务直接相关的代码。不顺手重构、不改无关格式、不"顺便优化"。
3. **一切走分支**：从最新 `origin/master` 开 `alma/loop-<主题>` 分支，绝不允许直接 push master。
4. **PR 制**：每个改动开 PR，描述里写清：改了什么、为什么、验证结果（贴命令输出摘要）。
5. **不碰用户的 WIP**：用户主目录里的未提交改动不属于本循环。
6. **诚实汇报**：修不了就直说修不了，在 issue/PR 里说明原因。禁止假装完成。
7. **写 LESSONS**：每次踩坑（编译失败的原因、跑通的调试路径、上游的怪癖）必须追加到 `LESSONS.md`，一条一行，给下一个循环的 agent 看。

## 验证命令

```bash
pnpm build          # 前端 + tauri 构建（在仓库根目录）
cd src-tauri && cargo check   # Rust 快速检查
```

## Git 纪要

- 提交信息：`fix: ...` / `feat: ...` / `chore: ...`，一行说清
- push 前先 `git pull --rebase origin master`
- 仓库 remote: `origin`（OpenFuckJob/FuckJob），gh CLI 已登录，有 push/maintain 权限
