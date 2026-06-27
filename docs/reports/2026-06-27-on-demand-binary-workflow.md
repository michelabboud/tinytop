# On-Demand Cross-Platform Binary Workflow

Date: 2026-06-27
Version: 0.1.34

## Summary

TinyTop now has a manual GitHub Actions workflow for building release binaries on demand.

The workflow supports Linux, Windows, and macOS release assets without requiring a local machine for each platform.

## Workflow

- File: `.github/workflows/build-binaries.yml`
- Trigger: `workflow_dispatch`
- Inputs: `platform`, `release_tag`, `upload_to_release`
- Official action pins: `actions/checkout@v7` and `actions/upload-artifact@v7`

## Assets

- `tinytop-agent-linux-x86_64`
- `tinytop-agent-windows-x86_64.exe`
- `tinytop-agent-macos-x86_64`
- `tinytop-agent-macos-aarch64`

Each asset is paired with a `.sha256` checksum.

## Release upload

When `upload_to_release` is true, the workflow requires an existing `release_tag` and uploads assets with:

```bash
gh release upload "$release_tag" <asset> <asset>.sha256 --clobber
```

This makes release-asset regeneration explicit and repeatable.

## Verification

- Added `tests/github-actions-build-binaries.test.ts`.
- The targeted workflow test was first run in red state and failed because `.github/workflows/build-binaries.yml` did not exist.
- After adding the workflow, `bun test tests/github-actions-build-binaries.test.ts` passed with `3 pass`, `0 fail`.
- `./tinytop rust build`: rebuilt the embedded Rust agent as `0.1.34`.
- `./tinytop status`: confirmed the running daemon reports `rust collector-dashboard-daemon v0.1.34 (embedded dashboard)`.
- `bun run check`: passed with `88 pass`, `0 fail`.
- `git diff --check`: passed.
- `bun audit`: no vulnerabilities found.
- `cargo audit`: scanned 196 crate dependencies with no vulnerabilities reported.
- `gh api repos/actions/checkout/releases/latest --jq '.tag_name'`: returned `v7.0.0`.
- `gh api repos/actions/upload-artifact/releases/latest --jq '.tag_name'`: returned `v7.0.1`.
- Local Linux bootstrap asset checksum: `e3d35d633836492a40ba47932178b9cd7bb00da57b5f80042e78f813f19bad7b`.
