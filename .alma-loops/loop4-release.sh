#!/usr/bin/env bash
# Loop 4 — 每日自动发版（有改动才发）
# 逻辑: 对比最新 tag/Release 与 origin/master → 有新提交则: 升版本号(三处) → 打 tag → push 触发 release.yml
#       无新提交则: 记一条日志睡觉
# 用法: ./.alma-loops/loop4-release.sh [max-iterations]
# 依赖: gh CLI（已登录）、git push 权限、release.yml 由 v* tag 触发
set -uo pipefail
cd "$(dirname "$0")/.."

MAX_ITERS="${1:-9999}"
LOG=".alma-loops/loop4.log"
INTERVAL=86400   # 24 小时
echo "=== Loop4(每日发版) 启动 $(date '+%F %T') ===" | tee -a "$LOG"

for i in $(seq 1 "$MAX_ITERS"); do
  echo "--- 巡检 $i/$MAX_ITERS $(date '+%F %T') ---" | tee -a "$LOG"

  # 1) 拿最新 release 的 tag（以 GitHub Release 为准，而不是本地 git tag——历史断过）
  LAST_TAG=$(gh release list -R OpenFuckJob/FuckJob --limit 1 --json tagName --jq '.[0].tagName' 2>/dev/null)
  LAST_TAG=${LAST_TAG:-v0.1.9}
  echo "[发版] 最新 Release: $LAST_TAG" | tee -a "$LOG"

  timeout 60 git fetch origin master -q
  # 2) 上次发版后 master 有没有新提交？
  NEW_COMMITS=$(git rev-list "$LAST_TAG..origin/master" --count 2>/dev/null || git rev-list "origin/master" --since="24 hours ago" --count)
  if [ "$NEW_COMMITS" = "0" ] || [ -z "$NEW_COMMITS" ]; then
    echo "[发版] master 无新提交（$LAST_TAG 之后 0 commit），今日不发版" | tee -a "$LOG"
    sleep "$INTERVAL"
    continue
  fi
  echo "[发版] 检测到 $NEW_COMMITS 个新提交，开始发版流程" | tee -a "$LOG"

  # 3) 交给 agent 干活：升版本号 + 构建验证 + 打 tag
  timeout 1800 codex exec \
    --skip-git-repo-check \
    -s workspace-write \
    -c model_provider="magpie" \
    -c model="zai/glm-5.3-flash" \
    -c approval_policy="never" \
    --dangerously-bypass-approvals-and-sandbox \
    "$(cat << PROMPT
你在 FuckJob 仓库执行每日发版任务。严格按以下步骤，任何一步失败就停止并如实报告：

1. git fetch origin master && git checkout origin/master --detach（以最新 master 为基础）
2. 用 gh release list 确认最新 Release tag（记为 LAST），git log LAST..origin/master --oneline 列出本次发版包含的提交
3. 版本号规则：目标 = max(最新Release tag 的版本, 源码现有版本) + 1 patch。例：Release=v0.1.22 但源码已是 0.1.29 → 目标 0.1.30（绝不允许降版本，若算出的目标 < 源码版本立即停止报告）。同步改三处：src-tauri/tauri.conf.json、src-tauri/Cargo.toml、package.json 的 version 字段
4. cd src-tauri && cargo check 全绿（会更新 Cargo.lock）；仓库根目录 pnpm build 全绿
5. commit（消息: "chore: release v<X.Y.Z>"，三处版本号 + Cargo.lock），打 annotated tag "v<X.Y.Z>"（message: "OfferFlow v<X.Y.Z>"）
6. git push origin HEAD:master 然后 git push origin v<X.Y.Z>（tag 推送会触发 release.yml 三平台构建）
7. 等 2 分钟后用 gh run list 查 release workflow 是否启动，输出本次发版摘要：版本号、包含的提交、构建状态

注意：绝不能在构建没全绿时打 tag；推送 master 失败（分支保护）就停下报告，不要 force。
PROMPT
    )" 2>&1 | tee -a "$LOG"

  echo "--- 巡检 $i 完成，休眠 24h ---" | tee -a "$LOG"
  sleep "$INTERVAL"
done
