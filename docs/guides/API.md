# API Guide

This guide documents the local HTTP APIs used by TinyTop.

## Base URLs

| Process | URL | Audience |
| --- | --- | --- |
| Rust collector/dashboard daemon | `http://127.0.0.1:4274` | Browser, local user, and legacy collector API clients |
| Legacy Bun collector | `http://127.0.0.1:4276` | Internal dashboard process when using Bun split mode |

Most TinyTop APIs are `GET` requests. `PUT /api/settings` is the supported write endpoint for daemon dashboard defaults. Unsupported methods return JSON errors with HTTP `405`.

## Public Dashboard API

### GET /health

Health check for the Rust collector/dashboard daemon or legacy Bun dashboard process.

Response:

```text
ok
```

### GET /api/version

Identifies the dashboard-serving runtime and product version. Use this when checking whether the new Rust collector/dashboard daemon or the legacy Bun dashboard is serving `127.0.0.1:4274`.

Example:

```bash
curl -fsS http://127.0.0.1:4274/api/version
```

Rust response:

```json
{
  "status": "ok",
  "app": "tinytop",
  "version": "0.1.25",
  "runtime": "rust",
  "component": "collector-dashboard-daemon",
  "dashboard": "embedded"
}
```

Legacy Bun response:

```json
{
  "status": "ok",
  "app": "tinytop",
  "version": "0.1.25",
  "runtime": "legacy-bun",
  "component": "dashboard",
  "dashboard": "legacy",
  "collector": {
    "status": "ok",
    "app": "tinytop",
    "version": "0.1.25",
    "runtime": "legacy-bun",
    "component": "collector",
    "dashboard": "none"
  }
}
```

### GET /api/snapshot

Returns the latest `SystemSnapshot`. In the Rust daemon this is handled in-process. In Bun split mode it proxies to collector `/snapshot/latest`.

Example:

```bash
curl -fsS http://127.0.0.1:4274/api/snapshot
```

Response shape:

```json
{
  "timestamp": "2026-06-24T10:15:46.568Z",
  "identity": {
    "hostname": "devbox",
    "platform": "linux",
    "arch": "x64",
    "distro": "Ubuntu 24.04.4 LTS",
    "kernel": "6.18.33.1-microsoft-standard-WSL2",
    "runtime": {
      "kind": "WSL",
      "confidence": "high",
      "reason": "kernel release/version contains Microsoft WSL markers"
    },
    "uptimeSeconds": 246000
  },
  "cpu": {
    "usagePercent": 4.8,
    "cores": 28,
    "times": {}
  },
  "memory": {
    "totalBytes": 0,
    "availableBytes": 0,
    "usedBytes": 0,
    "usedPercent": 0
  },
  "swap": {
    "totalBytes": 0,
    "freeBytes": 0,
    "usedBytes": 0,
    "usedPercent": 0
  },
  "load": {
    "one": 0,
    "five": 0,
    "fifteen": 0,
    "runnable": 0,
    "totalThreads": 0,
    "lastPid": 0
  },
  "pressure": {
    "cpu": {},
    "memory": {},
    "io": {}
  },
  "filesystems": [],
  "processes": []
}
```

The example above is shortened. Real responses include full CPU times, filesystem rows, pressure data when present, and process rows.

### GET /api/settings

Returns dashboard defaults from the Rust daemon's SQLite `app_settings` table. In legacy Bun fallback mode the dashboard exposes the same shape in memory so the UI remains usable, but durable settings are owned by the Rust daemon.

Example:

```bash
curl -fsS http://127.0.0.1:4274/api/settings
```

Response:

```json
{
  "defaultTheme": "midnight",
  "defaultGraphMode": "line",
  "pollIntervalMs": 1500,
  "defaultHistoryWindow": "live",
  "retentionHours": 72,
  "rollupRetentionDays": 30,
  "topProcessCount": 8,
  "redactionDefault": false,
  "thresholds": {
    "cpuWarn": 80,
    "cpuCritical": 95,
    "memoryWarn": 85,
    "memoryCritical": 95,
    "diskWarn": 85,
    "diskCritical": 95,
    "loadWarn": 80,
    "loadCritical": 100,
    "pressureWarn": 10,
    "pressureCritical": 25
  },
  "enabledSections": {
    "overview": true,
    "history": true,
    "filesystem": true,
    "pressure": true,
    "processes": true
  }
}
```

### PUT /api/settings

Persists daemon dashboard defaults. The payload must use the same shape returned by `GET /api/settings`. Invalid enum values or out-of-range numbers return HTTP `400`.

Example:

```bash
curl -fsS -X PUT http://127.0.0.1:4274/api/settings \
  -H 'content-type: application/json' \
  --data '{"defaultTheme":"aurora","defaultGraphMode":"heatmap","pollIntervalMs":3000,"defaultHistoryWindow":"1h","retentionHours":96,"rollupRetentionDays":30,"topProcessCount":12,"redactionDefault":false,"thresholds":{"cpuWarn":80,"cpuCritical":95,"memoryWarn":85,"memoryCritical":95,"diskWarn":85,"diskCritical":95,"loadWarn":80,"loadCritical":100,"pressureWarn":10,"pressureCritical":25},"enabledSections":{"overview":true,"history":true,"filesystem":true,"pressure":true,"processes":true}}'
```

The Settings dialog separates browser-local choices from daemon defaults:

| Scope | Storage |
| --- | --- |
| Active theme, graph mode, history window, visible series, process table state, filesystem system-mount toggle, and last section for this browser | `localStorage` |
| Default theme, graph mode, refresh interval, retention/rollup defaults, warning/critical thresholds, and enabled sections | SQLite `app_settings` |

### GET /api/history

Returns persisted recent history from the Rust daemon or legacy Bun collector process. The query parameters bound the read result only; they do not prune SQLite history.

The dashboard timeline uses the explicit `since_ms` and `until_ms` parameters for its Live, 15m, 1h, 6h, and 24h range presets. Larger ranges may be paged by issuing another request with `until_ms` set before the oldest sample already returned.

Query parameters:

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `limit` | integer | `120` | Maximum number of samples returned by this request; clamped to `1..10000` |
| `window_seconds` | integer | collector default `300` | Relative time window when `since_ms` is absent |
| `since_ms` | integer | derived from `window_seconds` | Inclusive Unix epoch millisecond lower bound |
| `until_ms` | integer | none | Inclusive Unix epoch millisecond upper bound |

Example:

```bash
curl -fsS 'http://127.0.0.1:4274/api/history?limit=3&window_seconds=300'
```

Response:

```json
{
  "samples": [
    {
      "capturedAtMs": 1782296146568,
      "snapshot": {
        "timestamp": "2026-06-24T10:15:46.568Z"
      }
    }
  ]
}
```

Samples are returned oldest first.

Retention note: The dashboard uses explicit `since_ms` and `until_ms` windows for its range presets, while the API default window is 300 seconds when no explicit window is supplied. In the Rust daemon, raw rows are pruned by the saved `retentionHours` setting after successful collection or settings update. The legacy Bun split path keeps raw SQLite rows until manual archive/reset.

### GET /api/history/coverage

Rust daemon endpoint that returns history coverage metadata for the dashboard rail. Legacy Bun split mode may return `404`; the dashboard handles that by showing unavailable coverage values.

Example:

```bash
curl -fsS http://127.0.0.1:4274/api/history/coverage
```

Response:

```json
{
  "sampleCount": 120,
  "oldestCapturedAtMs": 1782292546568,
  "newestCapturedAtMs": 1782296146568,
  "retentionHours": 72,
  "rollupRetentionDays": 30,
  "rollupBucketCount": 60,
  "databaseBytes": 1048576
}
```

### GET /vendor/echarts.min.js

Returns the vendored Apache ECharts browser bundle from the Rust embedded dashboard assets or, in legacy Bun mode, from `legacy/dashboard/vendor/echarts.min.js`.

### Static Assets

| Path | File |
| --- | --- |
| `/` | embedded `index.html` or `legacy/dashboard/index.html` |
| `/index.html` | embedded `index.html` or `legacy/dashboard/index.html` |
| `/styles.css` | embedded `styles.css` or `legacy/dashboard/styles.css` |
| `/app.js` | embedded `app.js` or `legacy/dashboard/app.js` |

## Legacy Collector API

### GET /health

Health check for the Rust daemon or legacy Bun collector process.

Response:

```text
ok
```

### GET /version

Identifies the collector-compatible API runtime. The Rust daemon exposes this on `127.0.0.1:4274`; the legacy Bun collector exposes it on `127.0.0.1:4276`.

Response:

```json
{
  "status": "ok",
  "app": "tinytop",
  "version": "0.1.25",
  "runtime": "rust",
  "component": "collector-dashboard-daemon",
  "dashboard": "embedded"
}
```

### GET /snapshot/latest

Returns the latest stored snapshot. If no sample exists yet, the collector collects and stores one before responding.

### GET /snapshot/collect

Collects a new live snapshot, stores it in SQLite, and returns it. The collector timer uses this route internally.

### GET /history

Returns persisted samples from SQLite. Query parameters match dashboard `/api/history`.

In the default Rust daemon, these legacy collector routes are available on
`http://127.0.0.1:4274`. In legacy Bun split mode they are available on
`http://127.0.0.1:4276`.

## Error Responses

Errors are JSON:

```json
{ "error": "message" }
```

Common status codes:

| Status | Meaning |
| --- | --- |
| `404` | Route not found |
| `405` | Non-GET method |
| `500` | Collection, collector, or history query failure |
| `503` | Provider missing in test/fallback handler configuration |

## Cache Headers

Dynamic API responses set:

```text
cache-control: no-store
```

Static assets are also served with `no-store` during local development so refreshes pick up current files.
