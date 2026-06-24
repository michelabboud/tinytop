# Dependency and Runtime Vetting

## Summary

The Bun dashboard uses the installed Bun runtime (`bun 1.3.11`) as the server, package runner, and test runner. The browser chart dependency is Apache ECharts. The optional Rust collector preview has separate dependency vetting in [2026-06-24-rust-agent-dependency-vetting.md](2026-06-24-rust-agent-dependency-vetting.md).

## Bun

- Version used: `1.3.11`, verified locally with `bun --version`.
- Documentation source: official Bun documentation via Context7 library `/oven-sh/bun`.
- APIs used: `Bun.serve()`, `Bun.file()`, `Bun.spawn()`, `Bun.sleep()`, and `bun test`.
- Reason selected: Bun fits the local dashboard well as runtime, package runner, and test runner.

## Playwright QA

Playwright was used only as an external temporary QA runner through `/tmp/tinytop-qa`; it was not added to this project. Chromium was installed into the user Playwright cache to run desktop/mobile rendered checks.

## Apache ECharts

- Version pinned by `package.json`: `^6.1.0`
- Purpose: renders Live History line, stacked area, stacked bar, heatmap, and treemap chart modes.
- Serving model: local `node_modules` bundle exposed through `/vendor/echarts.min.js`.

## Alternatives

- Node HTTP server: rejected because the project standardizes on Bun.
- React/Vite: rejected to keep the standalone dashboard dependency-free.
- Static-only HTML: rejected because it cannot provide real live WSL/Linux data.
