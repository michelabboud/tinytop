# Dependency and Runtime Vetting

## Summary

The dashboard has no project package dependencies. It uses the installed Bun runtime (`bun 1.3.11`) as the server, package runner, and test runner.

## Bun

- Version used: `1.3.11`, verified locally with `bun --version`.
- Documentation source: official Bun documentation via Context7 library `/oven-sh/bun`.
- APIs used: `Bun.serve()`, `Bun.file()`, `Bun.spawn()`, `Bun.sleep()`, and `bun test`.
- Reason selected: Michel explicitly requested Bun, and it fits the local single-process dashboard well.

## Playwright QA

Playwright was used only as an external temporary QA runner through `/tmp/tinytop-qa`; it was not added to this project. Chromium was installed into the user Playwright cache to run desktop/mobile rendered checks.

## Alternatives

- Node HTTP server: rejected because Michel asked to use Bun.
- React/Vite: rejected to keep the standalone dashboard dependency-free.
- Static-only HTML: rejected because it cannot provide real live WSL/Linux data.
