# BOSS Isolated Work Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep the BOSS job-list tab unchanged while each greeting is performed in a disposable work tab.

**Architecture:** The list tab remains the source of card order and progress. For every selected job, a new stealth-injected work tab opens the same search URL, locates the card by platform job ID, opens its right-side detail, performs greeting and chat navigation, sends the message, then closes. The list tab never participates in BOSS's JavaScript chat routing.

**Tech Stack:** Rust, Tauri, rust_drission, BOSS web UI.

---

### Task 1: Define work-tab targeting

**Files:**
- Modify: `src-tauri/src/rpa/boss/handler/position_say_hello.rs`
- Test: `src-tauri/src/rpa/boss/handler/position_say_hello.rs`

- [x] Write a failing test for matching a job card link to the platform job ID.
- [x] Run the focused Rust test and confirm it fails.
- [x] Add the minimal job-card selector/matcher used by the work tab.
- [x] Run the focused Rust test and confirm it passes.

### Task 2: Isolate greeting lifecycle

**Files:**
- Modify: `src-tauri/src/rpa/boss/handler/position_say_hello.rs`

- [x] Change `handle_greet` to receive the list URL and create a stealth-injected work tab.
- [x] In the work tab, wait for the card, click the matching job, and reuse the existing greeting/chat handling.
- [x] Always close the work tab after success or failure; leave the list tab untouched.
- [x] Run the BOSS handler tests and compile check.

### Task 3: Regression verification

**Files:**
- Modify: `docs/superpowers/plans/2026-08-05-boss-isolated-work-tab.md`

- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml`.
- [x] Run `cargo check --manifest-path src-tauri/Cargo.toml`.
- [x] Mark completed steps with verification results.
