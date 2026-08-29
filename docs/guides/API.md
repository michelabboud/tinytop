# API Guide

This guide documents the local HTTP APIs used by TinyTop.

## Base URLs

| Process | URL | Audience |
| --- | --- | --- |
| Rust collector/dashboard daemon on Linux/WSL | `http://127.0.0.1:4274` | Browser, local user, and legacy collector API clients |
| Rust collector/dashboard daemon on native Windows | `http://127.0.0.1:4275` | Browser and local Windows user |
| Legacy Bun collector | `http://127.0.0.1:4276` | Internal dashboard process when using Bun split mode |

Most TinyTop APIs are `GET` requests. `PUT /api/settings` is the supported write endpoint for daemon dashboard defaults. Unsupported methods return JSON errors with HTTP `405`.

## Public Dashboard API

### GET /health

Health check for the Rust collector/dashboard daemon or legacy Bun dashboard process.

Response:

```json
{
  "status": "ok",
  "app": "tinytop",
  "version": "0.2.0",
  "daemon": {
    "os": "windows",
    "arch": "x86_64",
    "install": {
      "executable": "C:\\Users\\michel\\AppData\\Local\\TinyTop\\bin\\tinytop-agent.exe",
      "workingDirectory": "C:\\Users\\michel\\repos\\tinytop"
    },
    "bind": {
      "host": "127.0.0.1",
      "port": 4275
    },
    "storage": {
      "sqliteUrl": "sqlite://C:\\Users\\michel\\AppData\\Local\\TinyTop\\state\\history.sqlite",
      "sqlitePath": "C:\\Users\\michel\\AppData\\Local\\TinyTop\\state\\history.sqlite"
    }
  }
}
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
  "version": "0.2.0",
  "runtime": "rust",
  "component": "collector-dashboard-daemon",
  "dashboard": "embedded",
  "daemon": {
    "os": "linux",
    "arch": "x86_64",
    "install": {
      "executable": "/home/michel/projects/tinytop/agent/target/release/tinytop-agent",
      "workingDirectory": "/home/michel/projects/tinytop"
    },
    "bind": {
      "host": "127.0.0.1",
      "port": 4274
    },
    "storage": {
      "sqliteUrl": "sqlite:///home/michel/.local/share/tinytop/history.sqlite",
      "sqlitePath": "/home/michel/.local/share/tinytop/history.sqlite"
    }
  }
}
```

Legacy Bun response:

```json
{
  "status": "ok",
  "app": "tinytop",
  "version": "0.2.0",
  "runtime": "legacy-bun",
  "component": "dashboard",
  "dashboard": "legacy",
  "collector": {
    "status": "ok",
    "app": "tinytop",
    "version": "0.2.0",
    "runtime": "legacy-bun",
    "component": "collector",
    "dashboard": "none",
    "daemon": {
      "os": "linux",
      "arch": "x64",
      "install": {
        "executable": "/home/michel/.bun/bin/bun",
        "workingDirectory": "/home/michel/projects/tinytop"
      },
      "bind": {
        "host": "127.0.0.1",
        "port": 4276
      },
      "storage": {
        "sqlitePath": "/home/michel/.local/share/tinytop/history.sqlite"
      }
    }
  }
}
```

### GET /api/snapshot

Returns the latest `SystemSnapshot`. The Rust daemon answers from the collection task's latest in-memory value; before the first collection it returns HTTP `503` with `{"error":"no snapshot yet"}` (the daemon normally collects once before binding). In Bun split mode it proxies to collector `/snapshot/latest`.

Example:

```bash
curl -fsS http://127.0.0.1:4274/api/snapshot
```

Response shape:

```json
{
  "timestamp": "2026-06-24T10:15:46.568Z",
  "filesystemsCapturedAtMs": 1782296146568,
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

The example above is shortened. `filesystemsCapturedAtMs` is Unix time in milliseconds and can be older than `timestamp` between filesystem checks. `cpu.times` is optional: it is present on the Linux collector and absent on the sysinfo-based macOS/Windows collectors. Filesystem rows and pressure data are included when present, `load.runnable`, `load.totalThreads`, and `load.lastPid` are omitted when their collector has no source, and `processes.length` is at most the configured `topProcessCount`.

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
  "targetDatabaseBytes": 134217728,
  "topProcessCount": 8,
  "redactionDefault": false,
  "retentionLadder": {
    "l1": { "keepDays": 3 },
    "l2": { "keepDays": 30 },
    "l3": { "enabled": true, "keepDays": 90 },
    "l4": { "enabled": true, "keepDays": 730 },
    "detailIntervalSec": 60,
    "processFastKeepHours": 24
  },
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

`topProcessCount` accepts `1`–`50` and becomes effective on the daemon's next collection tick.

### PUT /api/settings

Persists daemon dashboard defaults. The payload must use the same shape returned by `GET /api/settings`. Invalid enum values or out-of-range numbers return HTTP `400`.

Example:

```bash
curl -fsS -X PUT http://127.0.0.1:4274/api/settings \
  -H 'content-type: application/json' \
  --data '{"defaultTheme":"aurora","defaultGraphMode":"line","pollIntervalMs":3000,"defaultHistoryWindow":"7d","retentionHours":96,"rollupRetentionDays":45,"targetDatabaseBytes":268435456,"topProcessCount":12,"redactionDefault":false,"thresholds":{"cpuWarn":80,"cpuCritical":95,"memoryWarn":85,"memoryCritical":95,"diskWarn":85,"diskCritical":95,"loadWarn":80,"loadCritical":100,"pressureWarn":10,"pressureCritical":25},"enabledSections":{"overview":true,"history":true,"filesystem":true,"pressure":true,"processes":true}}'
```

The Settings dialog separates browser-local choices from daemon defaults:

| Scope | Storage |
| --- | --- |
| Active theme, graph mode, history window, visible series, process table state, filesystem system-mount toggle, and last section for this browser | `localStorage` |
| Default theme, graph mode, refresh interval, retention/rollup defaults, target DB budget, warning/critical thresholds, and enabled sections | SQLite `app_settings` |

### GET /api/history

Returns persisted recent history. The Rust daemon assembles each snapshot from typed tables; the legacy Bun collector remains a separate legacy runtime. The query parameters bound the read result only; they do not prune SQLite history.

The dashboard timeline uses explicit `since_ms` and `until_ms` bounds for its Live, 15m, and 1h raw-snapshot presets. The 6h, 24h, 7d, 30d, 90d, 1y, and All presets use one `/api/history/points?source=auto&limit=10000` request so the server selects the finest retained tier that fits the complete range.

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

Retention note: The API default window is 300 seconds when no explicit window is supplied. In Rust, `/api/history` is assembled from typed tables and returns assembleable L1 rows through the ladder horizon; `retentionHours` is only the derived L1 compatibility mirror. History snapshots omit `cpu.times` and every `pressure.*.{some,full}` line. `load.runnable`, `load.totalThreads`, and `load.lastPid` are absent keys—not `null`—when the collector has no source. The legacy Bun split path keeps raw SQLite rows until manual archive/reset.

### GET /api/history/points

Rust daemon endpoint that returns chart-ready metric points from L1 raw, L2 one-minute, L3 five-minute, L4 hourly, or the queryable archive. This is additive; `/api/history` still returns recent full raw snapshots.

Query parameters:

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `limit` | integer | `120` | Maximum number of points, clamped to `1..10000` |
| `window_seconds` | integer | `300` | Relative time window when `since_ms` is absent |
| `since_ms` | integer | derived from `window_seconds` | Inclusive lower bound |
| `until_ms` | integer | none | Inclusive upper bound |
| `source` | enum | `auto` | `auto`, `raw`, `rollup` (one minute), `5m`, `1h`, or `archive` |

Example:

```bash
curl -fsS 'http://127.0.0.1:4274/api/history/points?source=rollup&limit=720'
```

Response:

```json
{
  "points": [
    {
      "capturedAtMs": 1782296146568,
      "source": "rollup",
      "sampleCount": 2,
      "cpuUsagePercent": 20.0,
      "memoryUsedPercent": 40.0,
      "swapUsedPercent": 0.0,
      "loadPercent": 15.0,
      "rootUsedPercent": 73.0
    }
  ],
  "source": "rollup",
  "resolutionMs": 60000,
  "available": true
}
```

`source` reports the selected tier, `resolutionMs` reports its nominal bucket width, and `available:false` marks the intentionally empty archive response until queryable archive reads land.

### GET /api/history/filesystems

Rust daemon endpoint for typed filesystem history. Since schema v3, it returns the rows stored on change rather than one row per enumeration; `/api/history` separately uses those rows and mount-presence events to assemble each snapshot. It accepts inclusive `sinceMs` / `untilMs`, optional exact `mount`, and `limit` clamped to `1..10000`; snake_case time aliases are also accepted. The response is `{ "filesystems": [...] }`, with each row containing `capturedAtMs`, `mount`, `filesystem`, `type`, byte/usage fields, and nullable inode fields.

### GET /api/history/processes

Rust daemon endpoint for typed process history. It accepts inclusive `sinceMs` / `untilMs` and a capture-group `limit` clamped to `1..10000`; snake_case time aliases are also accepted. The response is `{ "source": "fast" | "minute", "captures": [{ "capturedAtMs": ..., "processes": [...] }] }`; each complete capture preserves rank, PID, command, CPU/memory percentages, RSS bytes, and nullable parent/start fields.

```bash
curl -fsS 'http://127.0.0.1:4274/api/history/processes?sinceMs=1782292546568&limit=1'
```

Response:

```json
{
  "source": "fast",
  "captures": [
    {
      "capturedAtMs": 1782296146568,
      "processes": [
        {
          "rank": 0,
          "pid": 4242,
          "command": "tinytop-agent serve",
          "cpuPercent": 12.5,
          "memoryPercent": 1.2,
          "rssBytes": 67108864,
          "parentPid": 1,
          "startedAt": "2026-06-24T10:00:00Z"
        }
      ]
    }
  ]
}
```

The response uses `fast` only when `sinceMs` is present and falls within `processFastKeepHours` of the current time; an open-ended or older window uses `minute`.

### GET /api/history/markers

Rust daemon endpoint that returns durable timeline markers from daemon events and computed coverage gaps.

Marker types:

- `daemonStart`
- `settingsChange`
- `coverageGap`

Example:

```bash
curl -fsS 'http://127.0.0.1:4274/api/history/markers?limit=50&expected_gap_ms=60000'
```

Response:

```json
{
  "markers": [
    {
      "occurredAtMs": 1782296146568,
      "markerType": "settingsChange",
      "label": "Settings changed",
      "details": { "changed": ["targetDatabaseBytes"] }
    }
  ]
}
```

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
  "databaseBytes": 1048576,
  "targetDatabaseBytes": 134217728,
  "databaseBudgetPercent": 0.78,
  "rollupOldestCapturedAtMs": 1782292546568,
  "rollupNewestCapturedAtMs": 1782296146568,
  "tiers": [
    { "tier": "l1", "enabled": true, "keepDays": 3, "resolutionMs": 1500, "bucketCount": 120, "oldestMs": 1782292546568, "newestMs": 1782296146568 },
    { "tier": "l2", "enabled": true, "keepDays": 30, "resolutionMs": 60000, "bucketCount": 60, "oldestMs": 1782292546568, "newestMs": 1782296146568 },
    { "tier": "l3", "enabled": true, "keepDays": 90, "resolutionMs": 300000, "bucketCount": 12, "oldestMs": 1782292546568, "newestMs": 1782296146568 },
    { "tier": "l4", "enabled": true, "keepDays": 730, "resolutionMs": 3600000, "bucketCount": 1, "oldestMs": 1782292546568, "newestMs": 1782296146568 }
  ],
  "detailIntervalSec": 60,
  "disk": { "freeBytes": null, "minFreeBytes": 5368709120, "pressure": false, "lastCheckMs": null },
  "archive": {
    "queryable": { "enabled": false, "path": "history-archive.sqlite", "bucketCount": 0, "oldestMs": null, "newestMs": null },
    "cold": { "enabled": false, "directory": "", "exportedUntilMonth": null, "fileCount": 0, "bytes": 0 }
  },
  "migration": null
}
```

`detailIntervalSec` is the filesystem check interval in seconds (`15`–`3,600`); cached filesystem rows are served between checks.

### GET /vendor/echarts.min.js

Returns `agent/assets/dashboard/vendor/echarts.min.js`, embedded by Rust and served from the same single-source dashboard tree by Bun.

### Static Assets

| Path | File |
| --- | --- |
| `/` | `agent/assets/dashboard/index.html`, embedded by Rust |
| `/index.html` | `agent/assets/dashboard/index.html`, embedded by Rust |
| `/styles.css` | `agent/assets/dashboard/styles.css`, embedded by Rust |
| `/app.js` | `agent/assets/dashboard/app.js`, embedded by Rust |
| `/ladder-rules.js` | shared dashboard ladder helpers |

## Legacy Collector API

### GET /health

Health check for the Rust daemon or legacy Bun collector process.

Response:

```json
{
  "status": "ok",
  "app": "tinytop",
  "version": "0.2.0",
  "runtime": "rust",
  "component": "collector-dashboard-daemon",
  "dashboard": "embedded",
  "daemon": {
    "os": "linux",
    "arch": "x86_64",
    "install": {
      "executable": "/home/michel/projects/tinytop/agent/target/release/tinytop-agent",
      "workingDirectory": "/home/michel/projects/tinytop"
    },
    "bind": {
      "host": "127.0.0.1",
      "port": 4274
    },
    "storage": {
      "sqliteUrl": "sqlite:///home/michel/.local/share/tinytop/history.sqlite",
      "sqlitePath": "/home/michel/.local/share/tinytop/history.sqlite"
    }
  }
}
```

### GET /version

Identifies the collector-compatible API runtime. The Rust daemon exposes this on `127.0.0.1:4274`; the legacy Bun collector exposes it on `127.0.0.1:4276`.

Response:

```json
{
  "status": "ok",
  "app": "tinytop",
  "version": "0.2.0",
  "runtime": "rust",
  "component": "collector-dashboard-daemon",
  "dashboard": "embedded",
  "daemon": {
    "os": "linux",
    "arch": "x86_64",
    "install": {
      "executable": "/home/michel/projects/tinytop/agent/target/release/tinytop-agent",
      "workingDirectory": "/home/michel/projects/tinytop"
    },
    "bind": {
      "host": "127.0.0.1",
      "port": 4274
    },
    "storage": {
      "sqliteUrl": "sqlite:///home/michel/.local/share/tinytop/history.sqlite",
      "sqlitePath": "/home/michel/.local/share/tinytop/history.sqlite"
    }
  }
}
```

### GET /snapshot/latest

Uses the same handler and in-memory rule as `/api/snapshot`: it returns the latest snapshot published by the collection task, or HTTP `503` with `{"error":"no snapshot yet"}` before the first collection.

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
