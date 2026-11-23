# LESSONS — 踩坑教训库

> agent 每次踩坑后追加一条。格式：`- 日期 | 主题 | 教训`。循环启动时必须通读本文件。

- 2026-11-23 | loop 超时 | codex exec + glm-5.3-flash(effort=max) 跑一轮完整 issue 分诊需 10 分钟+，外层 shell timeout 必须给 ≥1800s，否则迭代被腰斩留下未提交 WIP
- 2026-11-23 | codex 接线 | codex 默认 provider 走 magpie 订阅代理链（openai_base_url），上游 502 时要显式 `-c model_provider="magpie"` 直连网关 127.0.0.1:3425/v1
- 2026-11-23 | pnpm 11 | 构建脚本放行需 `pnpm approve-builds esbuild`（package.json 的 pnpm 字段已废弃不读）；esbuild 不放行 vite 起不来
- 2026-11-23 | pnpm worktree | 嵌套 Git worktree 放在上层 pnpm workspace 下会把依赖装到父目录；功能 worktree 放 `/tmp` 或其他 workspace 外目录
- 2026-11-23 | 自评 PR | `gh pr review --request-changes` 不能对当前凭证自建的 PR 使用；只能留评论说明问题并等作者更新，不能自我 squash 合并
