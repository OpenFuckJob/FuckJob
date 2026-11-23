#!/usr/bin/env bash
# Loop 3 — 功能开发（从 issue #39「功能添加」等拉需求）
# 用法: ./.alma-loops/loop3-dev.sh [max-iterations]
set -uo pipefail
cd "$(dirname "$0")/.."

MAX_ITERS="${1:-1}"
LOG=".alma-loops/loop3.log"
echo "=== Loop3 启动 $(date '+%F %T') | repo: $(pwd) ===" | tee -a "$LOG"

for i in $(seq 1 "$MAX_ITERS"); do
  echo "--- 迭代 $i/$MAX_ITERS $(date '+%T') ---" | tee -a "$LOG"
  codex exec \
    --skip-git-repo-check \
    -s workspace-write \
    -c model_provider="magpie" \
    -c model="zai/glm-5.3-flash" \
    -c approval_policy="never" \
    --dangerously-bypass-approvals-and-sandbox \
    "$(cat << 'PROMPT'
你在 FuckJob 仓库的 alma/agent-loops worktree 里执行功能开发循环。严格按 AGENTS.md 铁律工作。

1. 先读 AGENTS.md 和 LESSONS.md。
2. 找一个待办需求：优先看 gh issue list 里标记了 enhancement 或"功能添加"类 issue（如 #39）下面可独立交付的小切片。选一个影响面最小、能一次 PR 交付的。
3. 开分支 alma/feat-<主题>，实现它。写代码前先读相关现有代码，遵循项目已有风格（Tauri 2: Rust 后端 + React 前端）。
4. 验证：pnpm build + cargo check 全绿。不绿不收工。
5. 开 PR：描述写清需求来源（issue 号）、改动点、验证结果。PR 标题加 [feat] 前缀。
6. 新教训追加进 LESSONS.md。输出本轮总结。

若本轮没有值得做的需求切片，明确输出"无待办，本轮空转"。绝不直接 push master。
PROMPT
    )" 2>&1 | tee -a "$LOG"
  echo "--- 迭代 $i 完成 $(date '+%T')，休眠 10 分钟 ---" | tee -a "$LOG"
  sleep 600
done
echo "=== Loop3 结束 $(date '+%F %T') ===" | tee -a "$LOG"
