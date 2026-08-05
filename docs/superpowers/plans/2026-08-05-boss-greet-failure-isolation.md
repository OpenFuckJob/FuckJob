# BOSS 建联失败隔离 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让单个 BOSS 岗位的建联或聊天失败只影响该岗位，并阻止无目标的通用聊天页跳转。

**Architecture:** 将岗位处理结果区分为成功、已沟通和可跳过失败。`handle_greet` 只在获得岗位专属会话地址或当前页已进入聊天状态时成功；外层列表循环将可跳过失败写入日志后继续下一岗位。把 redirect 校验与失败处置提取为纯函数以便单元测试。

**Tech Stack:** Rust、Tauri、rust-drission、Cargo tests。

---

### Task 1: 为跳转和失败处置建立回归测试

**Files:**
- Modify: `src-tauri/src/rpa/boss/handler/position_say_hello.rs`

- [x] **Step 1: 写出失败测试**

新增单元测试，断言空值、通用 `/web/geek/chat` 与不含目标岗位标识的地址都不能作为岗位聊天 redirect；断言单岗位失败的处置结果是继续处理。

- [x] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml boss::handler::position_say_hello::tests`

Expected: FAIL，因为 redirect 校验和失败处置尚未实现。

- [x] **Step 3: 实现最小辅助函数**

加入纯函数，仅接受含当前 `platform_job_id` 的 BOSS 聊天地址；把单岗位失败转成外层循环的 continue 决策。

- [x] **Step 4: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml boss::handler::position_say_hello::tests`

Expected: PASS。

### Task 2: 隔离单岗位建联失败

**Files:**
- Modify: `src-tauri/src/rpa/boss/handler/position_say_hello.rs:165-181,447-568`

- [x] **Step 1: 写出失败测试**

新增测试，断言只有已确认的岗位专属 redirect 才会产生聊天目标；不满足成功条件时返回带诊断信息的可跳过失败。

- [x] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml boss::handler::position_say_hello::tests`

Expected: FAIL，因为当前代码会回退打开通用聊天页。

- [x] **Step 3: 实现最小流程改动**

删除通用聊天页回退；点击与确认后重新读取按钮状态及 redirect。未得到成功信号时返回单岗位失败；外层仅记录 warning 并继续遍历卡片。

- [x] **Step 4: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml boss::handler::position_say_hello::tests`

Expected: PASS。

### Task 3: 全量验证

**Files:**
- Modify: `docs/superpowers/plans/2026-08-05-boss-greet-failure-isolation.md`

- [x] **Step 1: 运行完整 Rust 测试与编译检查**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && cargo check --manifest-path src-tauri/Cargo.toml`

Expected: 两个命令均以 0 退出。

- [x] **Step 2: 检查工作区差异**

Run: `git diff --check && git diff -- src-tauri/src/rpa/boss/handler/position_say_hello.rs`

Expected: 无空白错误，改动仅涵盖失败隔离与测试。
