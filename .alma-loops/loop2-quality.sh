#!/usr/bin/env bash
# Loop 2 — 质量巡逻（code review + AI slop 清理）
# 用法: ./.alma-loops/loop2-quality.sh [max-iterations]
set -uo pipefail
cd "$(dirname "$0")/.."

MAX_ITERS="${1:-1}"
LOG=".alma-loops/loop2.log"
echo "=== Loop2 启动 $(date '+%F %T') | repo: $(pwd) ===" | tee -a "$LOG"

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
你在 FuckJob 仓库的 alma/agent-loops worktree 里执行质量巡逻循环。严格按 AGENTS.md 铁律工作。

1. 先读 AGENTS.md 和 LESSONS.md。
2. 检查 git status 确认工作区干净（若 Loop1 留了半成品，跳过相关文件不动）。
3. 巡逻清单：
   a. 对比 origin/master 与本地，找最近的合入 commit（git log origin/master -20），抽查其中 2-3 个 diff，看有没有明显 bug、安全漏洞（硬编码密钥、注入）、或 AI slop（过度防御式编程、复制粘贴膨胀、无意义的注释噪音）。
   b. 检查 TODO/FIXME 里有没有该升级成 issue 的（gh issue create，标签 quality）。
   c. 跑一次 pnpm build + cargo check 确认主干健康；如果红了：开分支 alma/fix-ci-<日期>，修好，PR。
4. 有改动就走 PR 流程（开分支、验证、PR、描述里贴验证输出）。没毛病就明确说"本轮无发现"。
5. 新教训追加进 LESSONS.md。
6. 输出巡逻总结。

绝不直接 push master。
PROMPT
    )" 2>&1 | tee -a "$LOG"
  echo "--- 迭代 $i 完成 $(date '+%T')，休眠 10 分钟 ---" | tee -a "$LOG"
  sleep 600
done
echo "=== Loop2 结束 $(date '+%F %T') ===" | tee -a "$LOG"
