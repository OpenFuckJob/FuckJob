# Open-Source Local-First Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the first Apache-2.0 local-first release that has no dependency on the original business server, stores user data locally, and runs every AI feature through a user-configured OpenAI-compatible endpoint.

**Architecture:** Keep React as the UI and Tauri/Rust as the trusted local backend. Replace `server_api` with a typed `LlmService`, add versioned local configuration and OS-backed credentials, retain recruitment-platform browser traffic, and remove authentication, points, updater, and web-search code. Preserve JSON/YAML persistence while adding atomic writes, migration, and transactional backup/restore.

**Tech Stack:** React 19, TypeScript, Ant Design, Tauri 2, Rust 2021, reqwest, serde, keyring, tokio, Vitest, Testing Library.

**Spec:** `docs/superpowers/specs/2026-07-14-open-source-local-first-design.md`

---

## File and module map

### Rust backend

- `src-tauri/src/error.rs` — stable application error codes, safe user messages, optional technical details.
- `src-tauri/src/command/base.rs` — serializable `CommandResult<T>` and structured error payload.
- `src-tauri/src/config.rs` — versioned config schema, nullable LLM config, legacy parsing, browser-profile resolution.
- `src-tauri/src/storage/atomic.rs` — atomic same-directory file writes.
- `src-tauri/src/storage/backup.rs` — ZIP manifest, checksums, validation, restore and rollback.
- `src-tauri/src/storage/migration.rs` — idempotent resume/config/browser-path migrations.
- `src-tauri/src/credential.rs` — keychain/environment credential resolution without exposing plaintext.
- `src-tauri/src/llm/types.rs` — provider-neutral requests, responses, usage, connection reports.
- `src-tauri/src/llm/template.rs` — local `{{variable}}` expansion and validation.
- `src-tauri/src/llm/sse.rs` — chunk-safe OpenAI-compatible SSE decoder.
- `src-tauri/src/llm/openai_compatible.rs` — HTTP adapter for completions, streaming, model listing and connection tests.
- `src-tauri/src/llm/service.rs` — loads config/credential and exposes the provider to business callers.
- `src-tauri/src/command/llm_provider.rs` — credential, model-list and connection-test Tauri commands.
- `src-tauri/src/command/data.rs` — data-directory, backup, restore, log cleanup and reset commands.
- Existing `src-tauri/src/llm.rs`, `command/llm.rs`, `command/mock_interview.rs`, and `command/job.rs` — business call-site migration.

### React frontend

- `src/types/app-config.ts` — nullable LLM config, onboarding and schema version; no search config.
- `src/types/command.ts` — structured command errors and helpers.
- `src/types/llm.ts` — presets, credential status and connection report types.
- `src/lib/tauriConfig.ts` — config Tauri calls.
- `src/lib/llmConfig.ts` — secret/status/model/test Tauri calls.
- `src/hooks/useAppConfig.ts` — single config load/save/update state owner.
- `src/view/onboarding/index.tsx` — first-run privacy, browser and model setup.
- `src/view/config/LlmConfigPanel.tsx` — reusable model configuration and test UI.
- `src/view/about-data/index.tsx` — version, license, privacy and local-data operations.
- `src/App.tsx` — local startup and navigation without auth/update gates.

### Removed runtime modules

- `src-tauri/src/server_api/`
- `src-tauri/src/command/auth.rs`
- `src-tauri/src/search.rs`
- `src/lib/tauriAuth.ts`
- `src/lib/updater.tsx`
- `src/view/auth/`
- Account/points/redeem UI in `src/view/account/`

---

### Task 1: Establish structured errors without changing behavior

**Files:**
- Create: `src-tauri/src/error.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/command/base.rs`
- Modify: `src/types/command.ts`
- Modify: every frontend call site found by `rg -n 'result\.error' src`
- Test: inline Rust tests in `src-tauri/src/error.rs` and TypeScript tests in `src/types/command.test.ts`

- [ ] **Step 1: Add the frontend test runner and write failing command-error tests**

Add `vitest`, `jsdom`, `@testing-library/react`, and `@testing-library/jest-dom` as dev dependencies. Add scripts `test` and `test:run` to `package.json`, plus `vitest.config.ts` and `src/test/setup.ts`.

Test these cases in `src/types/command.test.ts`:

```ts
expect(commandErrorMessage({ code: "configuration", message: "请配置模型", detail: null })).toBe("请配置模型");
expect(() => unwrap({ success: false, data: null, error: { code: "network", message: "连接失败", detail: null } })).toThrow("连接失败");
```

- [ ] **Step 2: Run the focused frontend test and verify failure**

Run: `pnpm test:run src/types/command.test.ts`  
Expected: FAIL because `CommandError` and `commandErrorMessage` do not exist.

- [ ] **Step 3: Add Rust error types and update the command result contract**

Implement stable lowercase codes:

```rust
pub enum AppErrorCode {
    Configuration, Credential, Network, Provider,
    Storage, Browser, Validation, Cancelled, Internal,
}

pub struct AppError {
    pub code: AppErrorCode,
    pub message: String,
    pub detail: Option<String>,
}
```

Make `CommandResult<T>.error` an `Option<AppError>`. Implement conversions for `&str`, `String`, `anyhow::Error`, keyring errors, IO errors and provider errors. Keep existing commands compiling by mapping legacy strings to `internal` until later tasks assign more precise codes.

- [ ] **Step 4: Update frontend error helpers and all current call sites**

Change the TypeScript contract to:

```ts
export interface CommandError { code: string; message: string; detail: string | null }
export interface CommandResult<T> { data: T | null; success: boolean; error: CommandError | null }
```

Replace `result.error || "fallback"` with `commandErrorMessage(result.error, "fallback")`. Do not leave object-to-string coercions.

- [ ] **Step 5: Verify and commit**

Run: `pnpm test:run src/types/command.test.ts`, `pnpm build`, `cargo test --manifest-path src-tauri/Cargo.toml command::base::tests`, and `cargo test --manifest-path src-tauri/Cargo.toml error::tests`  
Expected: all PASS.  
Commit: `refactor: add structured application errors`

### Task 2: Add versioned nullable LLM configuration and safe config writes

**Files:**
- Create: `src-tauri/src/storage/mod.rs`
- Create: `src-tauri/src/storage/atomic.rs`
- Create: `src-tauri/src/storage/migration.rs`
- Create: `src-tauri/src/credential.rs`
- Modify: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/command/user_resumes.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/resource/app_config.yaml`
- Modify: `src/types/app-config.ts`
- Modify: `src/App.tsx`
- Test: inline tests in `config.rs`, `credential.rs`, `storage/atomic.rs`, and `storage/migration.rs`

- [ ] **Step 1: Write failing config and atomic-write tests**

Cover:

```rust
assert_eq!(default_app_config().schema_version, 1);
assert!(!default_app_config().onboarding_completed);
assert!(default_app_config().llm_config.is_none());
```

Also test legacy blank LLM config becomes `None`, a complete legacy config is preserved, timeout is constrained to 10–600, and a failed temporary write leaves the original file unchanged. Retain the deprecated `search_config` field temporarily so `search.rs`, `command/job.rs`, `App.tsx`, and the existing config view continue to compile until Task 7/8.

- [ ] **Step 2: Run focused Rust tests and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml config::tests` and `cargo test --manifest-path src-tauri/Cargo.toml storage::atomic::tests`  
Expected: FAIL because the new fields/module do not exist.

- [ ] **Step 3: Implement the schema and remove search configuration**

Add:

```rust
pub struct AppRuntimeConfig {
    pub schema_version: u32,
    pub onboarding_completed: bool,
    pub llm_config: Option<LlmConfig>,
    // existing job/platform/greet/replay/browser/resume fields
}

pub struct LlmConfig {
    pub provider: LlmProviderPreset,
    pub base_url: String,
    pub model: String,
    pub timeout_seconds: u64,
}
```

Use `Option<LlmConfig>` / `LlmConfig | null` as the only unconfigured state. Reject incomplete non-null configs on save. Update the `App.tsx` empty config to `llm_config: null`, but do not remove `SearchConfig` or `updateSearch` yet.

- [ ] **Step 4: Implement atomic writes, credential core, and the complete v0-to-v1 migration**

Write serialized content to a unique sibling `.<name>.<uuid>.tmp`, call `sync_all`, then rename over the destination. Implement the credential backend and effective-source resolution here because config migration needs it. The single v0-to-v1 coordinator must complete all of these before writing `schema_version: 1`:

1. Parse and validate the new config representation.
2. Move a legacy plaintext `llm_config.api_key` into keychain; on keychain failure preserve the original config byte-for-byte and abort.
3. Apply the complete old/target `user_resumes.json` conflict matrix from the spec.
4. Resolve the browser profile: explicit path, existing legacy `app_data_dir/default`, otherwise `app_data_dir/browser-profile`; never move an existing profile.
5. Create timestamped backups, atomically write migrated files, and only then commit config with version 1.

Rerunning the coordinator must be harmless. New installs may start directly at version 1 because no legacy files exist.

Declare `storage` and `credential` in `lib.rs`. In Tauri setup, run the migration coordinator after resolving application paths but before `dao::init`, browser initialization that consumes profile paths, or any normal config/data writer.

- [ ] **Step 5: Verify and commit**

Run: `cargo test --manifest-path src-tauri/Cargo.toml config::tests`, `cargo test --manifest-path src-tauri/Cargo.toml storage::migration::tests`, `cargo test --manifest-path src-tauri/Cargo.toml credential::tests`, and `pnpm build`  
Expected: PASS.  
Commit: `feat: add versioned local model configuration`

### Task 3: Add credential storage and effective-source reporting

**Files:**
- Create: `src-tauri/src/command/llm_provider.rs`
- Modify: `src-tauri/src/command/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src/types/llm.ts`
- Create: `src/lib/llmConfig.ts`
- Test: command-level Rust tests; `src/lib/llmConfig.test.ts`

- [ ] **Step 1: Extend the credential tests with command-level behavior**

Test keychain overrides environment, missing keychain falls back to `FUCKJOB_LLM_API_KEY`, blank values are ignored, no credential is valid for local providers, and clearing keychain reveals an existing environment source.

- [ ] **Step 2: Run and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml command::llm_provider::tests`  
Expected: FAIL because the Tauri-facing command module does not exist.

- [ ] **Step 3: Implement non-readable credential commands**

Expose only:

```text
get_llm_credential_status -> { configured, source: keychain|environment|none }
set_llm_api_key(api_key)   -> status
clear_llm_api_key          -> status after deletion
```

Never return plaintext. Use keyring service `fuck_job`, user `llm_api_key`. Map Linux secret-service failures to `credential` errors with the environment-variable fallback instruction.

- [ ] **Step 4: Add typed frontend wrappers and tests**

Mock Tauri `invoke` and verify the wrapper never expects an API key value and correctly displays the post-clear effective source.

- [ ] **Step 5: Verify and commit**

Run: `cargo test --manifest-path src-tauri/Cargo.toml credential::tests`, `pnpm test:run src/lib/llmConfig.test.ts`, and `pnpm build`  
Expected: PASS.  
Commit: `feat: store model credentials locally`

### Task 4: Build provider-neutral LLM types, templates, and SSE parsing

**Files:**
- Create: `src-tauri/src/llm/types.rs`
- Create: `src-tauri/src/llm/template.rs`
- Create: `src-tauri/src/llm/sse.rs`
- Modify: `src-tauri/src/llm.rs`
- Test: inline module tests

- [ ] **Step 1: Write failing template tests**

Verify all legacy variables, repeated placeholders, Unicode content, missing required values, and unknown placeholders. Unknown/missing variables must return `validation`, never an empty substitution.

- [ ] **Step 2: Write failing SSE decoder tests**

Cover one frame split across chunks, multiple frames in one chunk, CRLF separators, role-only deltas, content deltas, `[DONE]`, provider error JSON, malformed JSON and a final partial frame.

- [ ] **Step 3: Run focused tests and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml llm::template::tests` and `cargo test --manifest-path src-tauri/Cargo.toml llm::sse::tests`  
Expected: FAIL because the modules do not exist.

- [ ] **Step 4: Implement domain types, deterministic template expansion, and the decoder**

Use `LlmMessage`, `LlmRequest`, `LlmResponse`, `LlmUsage` and `ConnectionReport` from the spec. Keep parsers pure; no HTTP or Tauri handles in these files.

- [ ] **Step 5: Verify and commit**

Run: `cargo test --manifest-path src-tauri/Cargo.toml llm::template::tests` and `cargo test --manifest-path src-tauri/Cargo.toml llm::sse::tests`  
Expected: PASS.  
Commit: `feat: add local llm protocol primitives`

### Task 5: Implement the OpenAI-compatible HTTP provider

**Files:**
- Create: `src-tauri/src/llm/openai_compatible.rs`
- Create: `src-tauri/src/llm/service.rs`
- Modify: `src-tauri/src/llm.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/src/command/llm_provider.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline tests using a local loopback mock server

- [ ] **Step 1: Write failing non-streaming provider tests**

Assert URL normalization, bearer auth only when a key exists, model/message request JSON, timeout behavior, success parsing, 401, 404-model, 429, 500 and malformed response mapping.

- [ ] **Step 2: Write failing streaming provider tests**

Serve deliberately split SSE chunks from a local `127.0.0.1` listener and verify ordered deltas, final content, disconnect errors and no partial success.

- [ ] **Step 3: Run tests and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml llm::openai_compatible::tests`  
Expected: FAIL because the provider is missing.

- [ ] **Step 4: Implement the provider and service factory**

Add only the reqwest/tokio features needed for streaming and the local test server. `LlmService::from_app_handle` must load `llm_config`, resolve the effective credential, and return `configuration` when config is null.

Implement `complete`, `stream`, optional `GET /models`, short completion connection test, and stream connection test. Sanitize Authorization and response bodies before logging.

- [ ] **Step 5: Register provider commands and verify**

Register `list_llm_models`, `test_llm_connection`, and `test_llm_stream_connection`.  
Run: `cargo test --manifest-path src-tauri/Cargo.toml llm::openai_compatible::tests`, `cargo test --manifest-path src-tauri/Cargo.toml credential::tests`, and `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`  
Expected: PASS.  
Commit: `feat: connect user configured llm providers`

### Task 6: Migrate every AI feature to `LlmService`

**Files:**
- Modify: `src-tauri/src/llm.rs`
- Modify: `src-tauri/src/command/llm.rs`
- Modify: `src-tauri/src/command/mock_interview.rs`
- Modify: `src-tauri/src/command/job.rs`
- Modify: `src-tauri/src/config.rs`
- Modify: Boss and Liepin callers returned by `codegraph explore "callers of generate_greet_text and generate_replay_text"`
- Test: existing inline tests plus new request-construction and failure-safety tests

- [ ] **Step 1: Write failing local request-construction tests**

For greet, reply, job analysis, resume prediction/optimization and mock interview, assert the final `LlmRequest.messages` contain locally expanded values and no unresolved `{{...}}` tokens.

- [ ] **Step 2: Write failing side-effect safety tests**

Assert provider failure or stream interruption returns an error and never invokes the RPA send callback. Assert configured text-template fallback remains explicit and no empty message is sent.

- [ ] **Step 3: Migrate non-streaming features**

Replace `GenerateBo`/`GenerateVo` usage with `LlmService.complete`. Remove points from return types. Preserve current prompt text and JSON output parsing.

- [ ] **Step 4: Migrate mock-interview streaming**

Use `LlmService.stream` and keep existing Tauri `mock_interview:delta|done|error` events. Emit `done` only after `[DONE]` or a valid clean stream end; discard partial content on provider errors.

- [ ] **Step 5: Verify and commit**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`  
Expected: PASS.  
Commit: `refactor: route ai features through local provider`

### Task 7: Remove the original server, web search, and obsolete crypto

**Files:**
- Delete: `src-tauri/src/server_api/`
- Delete: `src-tauri/src/search.rs`
- Delete: `src-tauri/src/command/auth.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/command/mod.rs`
- Modify: `src-tauri/src/command/job.rs`
- Modify: `src-tauri/src/dao/model.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Test: job-analysis compatibility tests

- [ ] **Step 1: Remove search from job analysis while preserving old data reads**

Stop building `web_search_context`. Keep `search_summary` and `search_sources` on `InterviewJobAnalysis` with serde defaults and write empty values for new records.

- [ ] **Step 2: Delete server/auth/search modules and unused dependencies**

Remove keyring token code, RuoYi/AES/RSA request code, client ID, points and auth commands. Keep the now-inert serialized `SearchConfig` for one more task so the current frontend can still load its expected shape; there is no network caller after `search.rs` and `command/job.rs` usage are gone. Remove `aes`, `ecb`, `rand`, `rsa`, and any now-unused crate only after `cargo machete`/`rg` confirmation. Keep `reqwest`, `keyring`, `sha2` and `urlencoding` if still referenced.

- [ ] **Step 3: Verify and commit**

Run: `cargo test --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`  
Expected: PASS. The updater endpoint still exists until Task 9, so the final network scan is intentionally not run yet.  
Commit: `refactor: remove hosted backend dependencies`

### Task 8: Build a typed frontend config layer

**Files:**
- Create: `src/lib/tauriConfig.ts`
- Create: `src/hooks/useAppConfig.ts`
- Create: `src/view/config/LlmConfigPanel.tsx`
- Modify: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/resource/app_config.yaml`
- Modify: `src/types/app-config.ts`
- Modify: `src/App.tsx`
- Modify: `src/view/config/index.tsx`
- Test: `src/hooks/useAppConfig.test.tsx`

- [ ] **Step 1: Write failing hook tests**

Mock `load_app_config`/`save_app_config`. Test initial loading, load error, immutable nested updates, save success, save failure, and that `llm_config: null` remains null until explicitly configured.

- [ ] **Step 2: Run and verify failure**

Run: `pnpm test:run src/hooks/useAppConfig.test.tsx`  
Expected: FAIL because the hook does not exist.

- [ ] **Step 3: Extract config ownership from `App.tsx`**

Move all load/save/import/export state into the hook and typed wrapper. Now that Task 7 removed the search network consumer, remove Rust and TypeScript `SearchConfig`, `updateSearch`, stale `use_custom`, and API-key fields from serializable config in the same commit.

- [ ] **Step 4: Split the oversized config view at the touched boundary**

Create `LlmConfigPanel.tsx`; leave unrelated job/reply/resume forms in the existing file. The panel receives a nullable config plus typed callbacks, not the entire app state.

- [ ] **Step 5: Verify and commit**

Run: `pnpm test:run src/hooks/useAppConfig.test.tsx && pnpm build`  
Expected: PASS.  
Commit: `refactor: centralize local app configuration`

### Task 9: Replace login startup with onboarding and local navigation

**Files:**
- Create: `src/view/onboarding/index.tsx`
- Create: `src/view/onboarding/onboarding.test.tsx`
- Create: `src/components/AiFeatureGate.tsx`
- Create: `src/components/AiFeatureGate.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/view/job-data/AnalysisReport.tsx`
- Modify: `src/view/conversation-debug/index.tsx`
- Modify: `src/view/resume-optimizer/index.tsx`
- Modify: `src/view/resume-optimizer/MockInterviewDrawer.tsx`
- Modify: `src/view/config/index.tsx`
- Delete: `src/lib/tauriAuth.ts`
- Delete: `src/lib/updater.tsx`
- Delete: `src/view/auth/`
- Delete: `src/view/account/index.tsx`
- Modify: `package.json`
- Modify: `pnpm-lock.yaml`
- Create: `scripts/check-network-boundary.mjs`
- Modify: `build.sh`
- Delete: `update.json`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Write failing onboarding tests**

Cover privacy text, browser detection, six provider presets, skip behavior (`onboarding_completed=true`, `llm_config=null`), key-required validation, connection success, connection failure and completion.

- [ ] **Step 2: Run and verify failure**

Run: `pnpm test:run src/view/onboarding/onboarding.test.tsx`  
Expected: FAIL because onboarding does not exist.

- [ ] **Step 3: Implement startup state machine and onboarding**

`App` renders loading, recoverable config error, onboarding, or main shell. It never calls `current_session` and never checks for updates. Use the shared `LlmConfigPanel` in guided mode.

Add `AiFeatureGate` and wire every AI trigger in job analysis, conversation debug, resume optimization, mock interview, and LLM-enabled greet/reply configuration. With `llm_config: null`, the control must explain that AI is optional and provide a callback that switches the app to Configuration → 大模型. Test configured and unconfigured states.

- [ ] **Step 4: Remove auth and updater dependencies**

Remove `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process`, Rust updater/process plugins, updater capabilities, endpoint, and update-artifact generation. Delete all auth imports/routes and command registrations. Delete the old account page in the same task so its `tauriAuth` import cannot break TypeScript; Task 11 adds the replacement About & Data page.

Remove `generate_update_json` and every call to it from `build.sh`, delete the checked-in `update.json`, and keep artifact collection only. Add `scripts/check-network-boundary.mjs` and `pnpm check:network` now that all first-party server/update/search code is gone. The scan covers runtime source, Tauri config, package manifests, and `build.sh`, while excluding docs, lockfiles, generated output, tests that assert the forbidden list, and the scan file itself.

- [ ] **Step 5: Verify and commit**

Run: `pnpm test:run src/view/onboarding/onboarding.test.tsx`, `pnpm test:run src/components/AiFeatureGate.test.tsx`, `pnpm build`, `cargo test --manifest-path src-tauri/Cargo.toml`, and `pnpm check:network`  
Expected: PASS.  
Commit: `feat: launch directly into local workspace`

### Task 10: Add a shared data lock, harden stores, cancellation, and log redaction

**Files:**
- Modify: `src-tauri/src/storage/atomic.rs`
- Modify: `src-tauri/src/storage/mod.rs`
- Modify: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/dao/store.rs`
- Modify: `src-tauri/src/command/user_resumes.rs`
- Modify: `src-tauri/src/logger.rs`
- Modify: `src-tauri/src/rpa/run_flow.rs`
- Modify: `src-tauri/src/command/rpa/run_flow.rs`
- Modify: `src-tauri/src/rpa/boss/handler/reply_unread.rs`
- Modify: `src-tauri/src/rpa/liepin/handler/reply_unread.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: inline storage, logger, RPA cancellation and send-safety tests

- [ ] **Step 1: Write failing shared-lock and atomic-store tests**

Test that config, DAO and resume writes all take a shared/read permit; an exclusive restore permit blocks each writer; and failed atomic writes preserve original content. Migration matrix coverage already lives in Task 2.

- [ ] **Step 2: Run and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml storage::tests`  
Expected: FAIL because the shared data lock is not integrated.

- [ ] **Step 3: Reuse atomic writes for all JSON stores**

Add one process-wide `RwLock` in `storage`: ordinary config/DAO/resume writes acquire a shared/read permit and restore acquires the exclusive/write permit. Replace `fs::write` in config, `JsonStore`, and user resumes with `storage::atomic::write`. Keep read formats unchanged.

- [ ] **Step 4: Implement cancellation mapping and sensitive-log redaction**

Map user-requested task stops to `AppErrorCode::Cancelled` instead of generic internal errors. Remove debug logging of full chat messages/runtime config, and add a centralized redaction helper for Authorization, API keys, cookies, resume content and chat bodies. Ensure Boss and Liepin LLM failures cannot reach send calls.

- [ ] **Step 5: Verify and commit**

Run: `cargo test --manifest-path src-tauri/Cargo.toml storage::tests`, `cargo test --manifest-path src-tauri/Cargo.toml logger::tests`, and `cargo test --manifest-path src-tauri/Cargo.toml rpa::run_flow::tests`  
Expected: PASS.  
Commit: `feat: harden local persistence and task safety`

### Task 11: Add transactional backup/restore and About & Data UI

**Files:**
- Create: `src-tauri/src/storage/backup.rs`
- Create: `src-tauri/src/command/data.rs`
- Modify: `src-tauri/src/storage/mod.rs`
- Modify: `src-tauri/src/command/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/dao/store.rs`
- Modify: `src-tauri/src/command/user_resumes.rs`
- Modify: `src-tauri/src/logger.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Create: `src/view/about-data/index.tsx`
- Create: `src/view/about-data/about-data.test.tsx`
- Modify: `src/App.tsx`
- Create: `LICENSE`

- [ ] **Step 1: Write failing backend backup tests**

Test manifest version, SHA-256, exact allowlisted entries, exclusion of keys/logs/browser profile, path traversal rejection, corrupt checksum, unsupported future schema, successful whole-package replace, and injected mid-restore failure with complete rollback.

- [ ] **Step 2: Run and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml storage::backup::tests`  
Expected: FAIL because backup support does not exist.

- [ ] **Step 3: Implement ZIP export and staged restore**

Add a ZIP crate. Export `manifest.json`, sanitized `config/app_config.yaml`, and the four allowlisted JSON files while holding Task 10's exclusive lock so the snapshot is consistent. Restore by extract/validate/migrate in a staging directory, snapshot current files, acquire the exclusive data-write lock, atomically replace, rollback all files on any error, and request a manual app restart after success.

Implement and register all data commands in this step:

```text
get_data_directory
export_data_backup(path)
restore_data_backup(path)
clear_logs
reset_app_config
```

`reset_app_config` preserves data and credentials, writes a fresh version-1 config atomically, and returns it. `clear_logs` runs under the shared lock and reports the affected path. The frontend uses `get_data_directory` plus the existing opener plugin to reveal it.

- [ ] **Step 4: Write failing About & Data tests, then implement the page**

Test Apache-2.0 display/link, privacy/network summary, data directory, backup, restore confirmation, log cleanup, key clear including environment fallback, and config reset confirmation.

- [ ] **Step 5: Add the standard Apache-2.0 license and wire navigation**

Use the unmodified Apache License 2.0 text in root `LICENSE`. Replace the account tab with “关于与数据”. Do not expose account/points/redeem language.

- [ ] **Step 6: Verify and commit**

Run: `cargo test --manifest-path src-tauri/Cargo.toml storage::backup::tests`, `pnpm test:run src/view/about-data/about-data.test.tsx`, and `pnpm build`  
Expected: PASS.  
Commit: `feat: add local data backup and privacy controls`

### Task 12: Finish safety regression tests and open-source documentation

**Files:**
- Modify: RPA send-path tests under `src-tauri/src/rpa/boss/` and `src-tauri/src/rpa/liepin/`
- Modify: `scripts/check-network-boundary.mjs`
- Rewrite: `README.md`
- Create: `docs/privacy-and-network.md`
- Create: `docs/model-configuration.md`
- Modify: `CrossLinux.Dockerfile`, `CrossWindows.Dockerfile` only if verification exposes release issues
- Modify: `.github/workflows/*` if workflows exist; otherwise document commands without inventing CI

- [ ] **Step 1: Add final regression tests**

Assert every automatic-send path requires a complete successful generation, cancelled tasks map to `cancelled`, logs redact secrets and sensitive bodies, credential fallback is visible, and restore rollback is complete. These behaviors were implemented in Tasks 2, 3, 10 and 11; Task 12 only closes missed coverage.

- [ ] **Step 2: Rewrite user and contributor documentation**

README must include: product scope, screenshots placeholder only if a real asset exists, supported platforms, no-account promise, exact network boundary, Ollama/LM Studio/online setup, privacy warning for remote models, local data paths, backup behavior, development commands, packaging commands, Apache-2.0 link and known limitations.

- [ ] **Step 3: Run the complete verification matrix**

Run:

```bash
pnpm test:run
pnpm build
pnpm check:network
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: all commands exit 0. The baseline before implementation is `pnpm build` PASS and `cargo test --manifest-path src-tauri/Cargo.toml` PASS with 86 tests.

- [ ] **Step 4: Run manual smoke tests**

Verify Ollama no-key completion and streaming; one authenticated OpenAI-compatible service; onboarding skip; AI-disabled states; Boss and Liepin login checks and a read-only collection flow; job analysis; resume optimization; mock interview; backup/restore; and legacy data migration. Record results in the PR/release notes without committing credentials or user data.

- [ ] **Step 5: Build release candidates on all three native platforms**

Cross-compiling only the Rust binary does not satisfy this gate. Run an equivalent native CI/release matrix:

```text
macOS runner:   ./build.sh macos
  require src-tauri/target/release/bundle/dmg/*.dmg
  require src-tauri/target/release/bundle/app/*.app

Linux runner:   ./build.sh linux
  require src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/appimage/*.AppImage
  require src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/deb/*.deb

Windows runner: ./build.sh windows
  require an NSIS installer under src-tauri/target/x86_64-pc-windows-gnu/release/bundle/nsis/*.exe
```

Each native runner must first execute the complete Step 3 verification matrix. On each runner, assert no generated `update.json` or updater signature exists in `releases/` or the bundle directories, run `pnpm check:network`, launch the packaged app for a smoke check where the runner permits it, and verify the About page contains Apache-2.0 attribution. A compile-only cross build may supplement but cannot replace a native packaging result.

- [ ] **Step 6: Final review and commit**

Run `git status --short` and inspect every remaining diff.  
Commit: `docs: prepare local-first open-source release`

---

## Execution notes

- Preserve the user's pre-existing `.codegraph/daemon.pid` modification and untracked `.superpowers/` directory; never stage them.
- Before editing an indexed subsystem, use CodeGraph first as required by `AGENTS.md`.
- Keep each task compiling and commit only the files listed for that task.
- Apply test-driven development for each behavior change: failing test, focused pass, broader verification, commit.
- If provider compatibility requires a nonstandard vendor extension, do not add it to this release; document the limitation and keep the OpenAI-compatible core strict.
