#!/usr/bin/env bash
# Loop 1 — 盯 GitHub issues + PR（yetone 式）
# 用法: ./.alma-loops/loop1-github.sh [max-iterations]
# 每轮: 分析 open issues → 修复 → PR → review 已有 PR → 关闭循环条件见 AGENTS.md
set -uo pipefail
cd "$(dirname "$0")/.."

MAX_ITERS="${1:-1}"
LOG=".alma-loops/loop1.log"
echo "=== Loop1 启动 $(date '+%F %T') | repo: $(pwd) ===" | tee -a "$LOG"

for i in $(seq 1 "$MAX_ITERS"); do
  echo "--- 迭代 $i/$MAX_ITERS $(date '+%T') ---" | tee -a "$LOG"
  timeout 1800 codex exec \
    --skip-git-repo-check \
    -s workspace-write \
    -c model_provider="magpie" \
    -c model="zai/glm-5.3-flash" \
    -c approval_policy="never" \
    --dangerously-bypass-approvals-and-sandbox \
    "$(cat << 'PROMPT'
你在 FuckJob 仓库的 alma/agent-loops worktree 里执行 GitHub 盯梢循环。严格按 AGENTS.md 铁律工作。

1. 先读 AGENTS.md 和 LESSONS.md。
2. 用 gh 命令列出所有 open issues（gh issue list --state open）和 open PRs（gh pr list --state open）。
3. 对每个 open issue：判断是否值得修复/实现。值得的：从最新 origin/master 开分支 alma/issue-<号>，修完跑 pnpm build 和 cargo check，全绿后 push 并开 PR（标题带 issue 号，正文写验证结果），然后在 issue 下评论"已在 #<PR号> 修复"。不值得修的：在 issue 下礼貌回复原因并关闭。
4. 对每个 open PR（非你自己开的）：gh pr diff + gh pr checkout 拉到本地实际构建（pnpm build + cargo check），审查代码逻辑有没有 bug、有没有无脑 AI slop。验证通过且逻辑正确才 gh pr merge（merge 方式用 squash）；有问题就 gh pr review --request-changes 写清楚问题，等作者改。
5. 所有修复完成后：把本次踩的新坑追加进 LESSONS.md（若有），git add+commit（消息 chore: update lessons）。
6. 输出本轮总结：修了什么、PR 号、关了什么、跳过了什么、为什么。

注意：git 操作前先 git status 确认没有别人的半成品；绝不直接 push master。
PROMPT
    )" 2>&1 | tee -a "$LOG"
  echo "--- 迭代 $i 完成 $(date '+%T')，休眠 5 分钟 ---" | tee -a "$LOG"
  sleep 300
done
echo "=== Loop1 结束 $(date '+%F %T') ===" | tee -a "$LOG"
