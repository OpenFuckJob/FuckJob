# CI/CD Design: GitHub Actions for Windows & macOS Builds

**Date:** 2026-07-22
**Project:** FuckJob (Tauri v2 desktop app)
**Status:** Approved

## Overview

Configure a single GitHub Actions workflow that:
1. Runs tests on every PR to `master`
2. Builds Windows and macOS installers on every push to `master`
3. Uploads build artifacts for download (90-day retention)

## Workflow Structure

```
Push/PR to master
    │
    ├── Job: test (ubuntu-latest)
    │      ├── Checkout
    │      ├── Setup pnpm + Node
    │      ├── pnpm install → tsc → vitest run
    │      ├── Setup Rust
    │      └── cargo test (in src-tauri/)
    │
    └── Job: build (matrix: macos-latest, windows-latest)
           (only on push, not PR — skip on pull_request)
           ├── Checkout
           ├── System deps (macOS: none; Windows: none via Tauri)
           ├── Setup pnpm + Node
           ├── pnpm install + pnpm build
           ├── Setup Rust + wasm target
           ├── tauri build --ci
           │      ├── macOS → .dmg
           │      └── Windows → .msi
           └── Upload artifact (dmg/msi)
```

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Test runner | `ubuntu-latest` | Cheapest, fastest runner for unit tests |
| Build runners | `macos-latest`, `windows-latest` | macOS uses Apple Silicon M1 runner |
| Bundle format | macOS `.dmg`, Windows `.msi` | Match existing `tauri.conf.json` targets |
| Code signing | None (skipped) | No Apple/Microsoft certs required for artifact distribution |
| Linux target | Removed | Remove `deb`, `appimage` from `tauri.conf.json` |
| Trigger | `push: master` + `pull_request: master` | Build on push, test on PR |
| Artifact retention | 90 days (default) | No GitHub Release, manual download |

## Workflow File

File: `.github/workflows/build.yml`

Single workflow with conditional job execution:
- `test` job: runs on both `push` and `pull_request`
- `build` job: runs only on `push` (uses `if: github.event_name == 'push'`)

## Tauri Config Changes

Remove Linux targets from `tauri.conf.json`:
- `bundle.targets`: keep only `["dmg", "app", "nsis"]`

## Artifacts

| Platform | Artifact | Description |
|----------|----------|-------------|
| macOS | `fuckJob_x.x.x_x64.dmg` | macOS disk image |
| Windows | `fuckJob_x.x.x_x64-setup.exe` | NSIS installer |

## Limitations

- **No code signing**: Users must bypass Gatekeeper (macOS) or SmartScreen (Windows) warnings
- **No auto-update**: No update server configured
- **No notarization**: macOS `.dmg` is not notarized by Apple
- **No GitHub Release integration**: Artifacts are ephemeral (90 days)

## Future Enhancements

- Add code signing with stored secrets (`APPLE_CERTIFICATE`, `WINDOWS_CERTIFICATE`)
- Add GitHub Release creation on tag push
- Add Linux builds if needed
