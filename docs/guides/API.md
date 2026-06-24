# API Guide

This guide documents the local HTTP APIs used by TinyTop.

## Base URLs

| Process | URL | Audience |
| --- | --- | --- |
| Rust daemon | `http://127.0.0.1:4274` | Browser, local user, and writer-compatible API clients |
| Legacy writer | `http://127.0.0.1:4276` | Internal dashboard process when using Bun split mode |

TinyTop APIs support only `GET` requests. Other methods return JSON errors with HTTP `405`.

## Public Dashboard API

### GET /health

Health check for the public dashboard process.

Response:

```text
ok
```

### GET /api/snapshot

Returns the latest `SystemSnapshot`. In the Rust daemon this is handled in-process. In Bun split mode it proxies to writer `/snapshot/latest`.

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

### GET /api/history

Returns persisted recent history from the Rust daemon or legacy writer process.

Query parameters:

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `limit` | integer | `120` | Maximum number of samples |
| `window_seconds` | integer | writer default `300` | Relative time window when `since_ms` is absent |
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

### GET /vendor/echarts.min.js

Returns the vendored Apache ECharts browser bundle from `public/vendor/echarts.min.js`.

### Static Assets

| Path | File |
| --- | --- |
| `/` | `public/index.html` |
| `/index.html` | `public/index.html` |
| `/styles.css` | `public/styles.css` |
| `/app.js` | `public/app.js` |

## Writer-Compatible API

### GET /health

Health check for the Rust daemon or legacy writer process.

Response:

```text
ok
```

### GET /snapshot/latest

Returns the latest stored snapshot. If no sample exists yet, the writer collects and stores one before responding.

### GET /snapshot/collect

Collects a new live snapshot, stores it in SQLite, and returns it. The writer timer uses this route internally.

### GET /history

Returns persisted samples from SQLite. Query parameters match dashboard `/api/history`.

In the default Rust daemon, these writer-compatible routes are available on
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
| `500` | Collection, writer, or history query failure |
| `503` | Provider missing in test/fallback handler configuration |

## Cache Headers

Dynamic API responses set:

```text
cache-control: no-store
```

Static assets are also served with `no-store` during local development so refreshes pick up current files.
