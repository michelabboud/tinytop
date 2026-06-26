# SVG Favicon Report

Date: 2026-06-26
Version: 0.1.28

## Summary

TinyTop now ships a local SVG favicon from both dashboard asset trees. The Rust embedded dashboard and legacy Bun dashboard remain byte-identical.

## Delivered

- Added `favicon.svg` under `legacy/dashboard/`.
- Added the matching `favicon.svg` under `agent/assets/dashboard/`.
- Replaced the blank favicon link in `index.html` with `/favicon.svg`.
- Added `/favicon.svg` to the Rust collector/dashboard static route and embedded asset allowlist.
- Served SVG assets as `image/svg+xml; charset=utf-8`.
- Expanded dashboard asset parity tests and Rust embedded serving tests for the favicon.

## Verification

- Red check before implementation:
  - `bun test tests/dashboard-assets.test.ts`: failed because `favicon.svg` did not exist in either dashboard tree.
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_exposes_embedded_dashboard_without_public_dir`: failed because `/favicon.svg` was not served as SVG.
- Green focused checks after implementation:
  - `bun test tests/dashboard-assets.test.ts`: `3 pass`, `0 fail`.
  - `cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_exposes_embedded_dashboard_without_public_dir`: `1 pass`, `0 fail`.
  - `diff -qr agent/assets/dashboard legacy/dashboard`: no differences.
- Full verification:
  - `./tinytop check`: Bun tests `76 pass`, `0 fail`, `387 expect() calls`; legacy server check, legacy collector check, Rust fmt, Rust workspace tests, and browser bundle build passed.
  - `cargo clippy --manifest-path agent/Cargo.toml --workspace --all-targets -- -D warnings`: clean.
  - `git diff --check`: clean.
  - `bun audit`: no vulnerabilities found.
  - `cargo audit --file agent/Cargo.lock`: no vulnerabilities reported.
- Release build:
  - `./tinytop rust build`: built `agent/target/release/tinytop-agent`.
  - Release binary SHA-256: `fb7f2fa3443fa27ecb4ce02632166eef5d72e52362445b515042f8060ee5d3a5`.
- Live smoke:
  - `./tinytop status`: reports `rust collector-dashboard-daemon v0.1.28 (embedded dashboard)`.
  - Live PID at report time: `1827235`.
  - `/api/version`: returned Rust `0.1.28`.
  - `/favicon.svg`: returned `HTTP/1.1 200 OK`, `content-type: image/svg+xml; charset=utf-8`, and the TinyTop SVG title.
