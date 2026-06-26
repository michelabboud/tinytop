# Dashboard Timeline And Settings Report

Date: 2026-06-26

## Scope

The approved dashboard improvement plan was saved at:

- `docs/superpowers/plans/2026-06-26-dashboard-timeline-settings.md`

This release implements the first vertical slice from that plan: make the current dashboard timeline timestamp-based and useful before introducing SQLite-backed daemon settings.

## Implemented In 0.1.21

- Added History range presets: Live, 15m, 1h, 6h, and 24h.
- Replaced `selectedSampleIndex` state with `selectedAtMs`.
- Changed history loading from the fixed `window_seconds` query to explicit `since_ms` and `until_ms` timestamp windows.
- Added client-side pagination for larger history ranges, using the existing `/api/history` limit contract.
- Fixed Rust history backfill so explicit empty `since_ms`/`until_ms` windows stay empty instead of falling back to the default latest-history window.
- Kept browser-local display preferences in localStorage and added `tinytop.historyWindow`.
- Kept `agent/assets/dashboard/` and `legacy/dashboard/` byte-identical.

## Verification

Full maintainer check:

```bash
./tinytop check
```

Result:

- Bun suite: `50 pass`, `0 fail`, `178 expect() calls`.
- `src/server.ts --check`: `status: ok`.
- `legacy/bun-collector.ts --check`: `status: ok`, in-memory DB.
- Rust fmt and workspace tests: passed.
- Browser bundle built successfully.

Focused tests:

```bash
bun test tests/dashboard-timeline.test.ts tests/dashboard-assets.test.ts tests/webui-dialogs.test.ts
cargo test --manifest-path agent/Cargo.toml -p tinytop-agent --test serve_contract serve_respects_empty_explicit_history_bounds
```

Result:

```text
dashboard timeline/assets/dialog tests: 9 pass, 0 fail, 42 expect() calls
Rust explicit empty history bounds test: 1 pass, 0 fail
```

Audits and diff hygiene:

```bash
bun audit
cargo audit --file agent/Cargo.lock
git diff --check
```

Result:

- `bun audit`: no vulnerabilities found.
- `cargo audit`: scanned `agent/Cargo.lock` for vulnerabilities across 196 crate dependencies.
- `git diff --check`: clean.

Browser bundle:

```bash
bun build legacy/dashboard/app.js --target=browser --outdir=/tmp/tinytop-dashboard-timeline-check
```

Result:

```text
Bundled 1 module
app.js  42.11 KB  (entry point)
```

Asset parity:

```bash
diff -qr agent/assets/dashboard legacy/dashboard
```

Result: no differences.

Rust embedded-dashboard smoke:

```bash
./tinytop rust build
PORT=4284 ./tinytop rust serve --sqlite sqlite::memory: --poll-ms 100000
curl -fsS http://127.0.0.1:4284/health
curl -fsS http://127.0.0.1:4284/app.js | rg "selectedAtMs|fetchHistoryPage|data-history-window|since_ms|until_ms"
curl -fsS http://127.0.0.1:4284/ | rg 'data-history-window="(live|15m|1h|6h|24h)"'
curl -fsS "http://127.0.0.1:4284/api/history?limit=5&since_ms=0"
```

Result:

- `/health` returned `ok`.
- Embedded `/app.js` contained `selectedAtMs`, `fetchHistoryPage`, `since_ms`, and `until_ms`.
- Embedded `/` contained all five history range buttons.
- `/api/history?limit=5&since_ms=0` returned SQLite-backed samples.
- The alternate-port smoke daemon was stopped after verification.

## Settings Roadmap

SQLite-backed daemon settings planned next:

- default theme
- default graph mode
- poll interval
- default history window
- retention hours
- rollup retention days
- top process count
- redaction default
- thresholds
- enabled sections

Browser-local settings that should stay in localStorage:

- active theme override
- active graph mode override
- selected history range
- panel expanded/collapsed state
- table sort/filter/search
- density/layout preference
- dismissed UI hints
- last visible section or scroll position

The next implementation slice should add the `app_settings` table, `GET /api/settings`, and `PUT /api/settings`, then expose a settings UI with separate `This Browser` and `This Daemon` sections.
