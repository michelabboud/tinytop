# TinyTop Integration Contract

TinyTop is a loopback-first host telemetry dashboard. External tools such as
tutus-remotus should reach it through host-local access they already control,
for example SSH-exec `curl 127.0.0.1:4274/api/...` for server-side telemetry
reads or an SSH/local tunnel for browser embedding.

TinyTop does not require public binding, browser CORS, bearer tokens, mTLS, or
outbound push for this integration contract. The default listener remains
`127.0.0.1`.

## Compatibility Commitment

The endpoints and fields documented here are the stable integration contract for
TinyTop `0.2.x`.

TinyTop may add fields at any time. It should not rename, remove, or retype the
documented fields without a major version bump.

## Capability Detection

`GET /api/version` advertises feature support:

```json
{
  "status": "ok",
  "app": "tinytop",
  "version": "0.2.0",
  "runtime": "rust",
  "component": "collector-dashboard-daemon",
  "dashboard": "embedded",
  "capabilities": ["snapshot", "history", "embed"]
}
```

Known capabilities:

| Capability | Meaning |
| --- | --- |
| `snapshot` | `GET /api/snapshot` is available |
| `history` | history APIs such as `GET /api/history/points` are available |
| `embed` | `GET /embed` serves the embeddable dashboard view |

## Embeddable Dashboard

`GET /embed` serves the same live dashboard in iframe-friendly mode. It removes
the TinyTop navigation/sidebar, top toolbar, and display controls while keeping
the telemetry panels, charts, filesystems, pressure, history, and process views.

Theme query parameters:

| Query | Result |
| --- | --- |
| `/embed?theme=dark` | maps to TinyTop's `midnight` palette |
| `/embed?theme=light` | maps to TinyTop's `solar` palette |
| `/embed?theme=midnight` | uses the named TinyTop palette |
| no `theme` query | uses the normal dashboard default/local preference |

Iframe permission is controlled by `TINYTOP_EMBED_FRAME_ANCESTORS`.

Default:

```bash
TINYTOP_EMBED_FRAME_ANCESTORS="'self'"
```

Example for tutus-remotus local UI:

```bash
TINYTOP_EMBED_FRAME_ANCESTORS="'self' http://127.0.0.1:9323"
```

Only `/embed` receives:

```text
Content-Security-Policy: frame-ancestors ...
```

The standalone dashboard keeps its existing behavior.

## GET /health

Health and identity endpoint.

Required stable fields:

| Field | Type | Notes |
| --- | --- | --- |
| `status` | string | `ok` when the daemon is healthy |
| `app` | string | always `tinytop` |
| `version` | string | product version |
| `capabilities` | string array | supported integration features |
| `daemon.os` | string | daemon operating system |
| `daemon.arch` | string | daemon architecture |
| `daemon.bind.host` | string | bound host |
| `daemon.bind.port` | number | bound port |
| `daemon.install.executable` | string | executable path |

## GET /api/version

Detection and runtime identity endpoint.

Required stable fields:

| Field | Type | Notes |
| --- | --- | --- |
| `status` | string | `ok` |
| `app` | string | always `tinytop` |
| `version` | string | product version |
| `runtime` | string | `rust` or `legacy-bun` |
| `component` | string | runtime component identity |
| `dashboard` | string | `embedded`, `directory`, `legacy`, `disabled`, or `none` |
| `capabilities` | string array | supported integration features |
| `daemon.os` | string | daemon operating system |
| `daemon.arch` | string | daemon architecture |
| `daemon.bind.host` | string | bound host |
| `daemon.bind.port` | number | bound port |
| `daemon.install.executable` | string | executable path |

## GET /api/snapshot

Live host telemetry.

Required stable top-level fields:

| Field | Type | Notes |
| --- | --- | --- |
| `timestamp` | string | ISO timestamp |
| `identity` | object | host identity |
| `cpu` | object | CPU usage |
| `memory` | object | memory usage |
| `swap` | object | swap usage |
| `load` | object | load and runnable/thread counts |
| `pressure` | object | PSI data; empty on platforms without PSI |
| `filesystems` | array | mounted filesystems |
| `processes` | array | top-N process list |

Stable identity fields:

| Field | Type |
| --- | --- |
| `identity.hostname` | string |
| `identity.platform` | string |
| `identity.arch` | string |
| `identity.distro` | string |
| `identity.kernel` | string |
| `identity.runtime.kind` | string |
| `identity.runtime.confidence` | string |
| `identity.runtime.reason` | string |
| `identity.uptimeSeconds` | number |

Stable metric fields:

| Field | Type |
| --- | --- |
| `cpu.usagePercent` | number |
| `cpu.cores` | number |
| `cpu.times` | object, optional by platform |
| `memory.totalBytes` | number |
| `memory.availableBytes` | number |
| `memory.usedBytes` | number |
| `memory.usedPercent` | number |
| `swap.totalBytes` | number |
| `swap.freeBytes` | number |
| `swap.usedBytes` | number |
| `swap.usedPercent` | number |
| `load.one` | number |
| `load.five` | number |
| `load.fifteen` | number |
| `load.runnable` | number |
| `load.totalThreads` | number |
| `load.lastPid` | number |

Stable filesystem item fields:

| Field | Type |
| --- | --- |
| `filesystem` | string |
| `type` | string |
| `sizeBytes` | number |
| `usedBytes` | number |
| `availableBytes` | number |
| `usedPercent` | number |
| `mount` | string |
| `inodeUsedPercent` | number or null |
| `inodeUsed` | number or null |
| `inodeTotal` | number or null |

Stable process item fields:

| Field | Type |
| --- | --- |
| `pid` | number |
| `command` | string |
| `cpuPercent` | number |
| `memoryPercent` | number |
| `rssBytes` | number |
| `parentPid` | number or null |
| `startedAt` | string, optional |

## GET /api/history/points

Sparkline and chart history endpoint.

Query parameters:

| Parameter | Type | Notes |
| --- | --- | --- |
| `limit` | number | maximum returned points |
| `since_ms` | number | inclusive lower bound, Unix milliseconds |
| `until_ms` | number | inclusive upper bound, Unix milliseconds |
| `window_seconds` | number | relative window fallback when explicit bounds are absent |
| `source` | string | `auto`, `raw`, or `rollup` |

Response shape:

```json
{
  "points": [
    {
      "capturedAtMs": 1760000000000,
      "cpuUsedPercent": 12.5,
      "memoryUsedPercent": 50.1,
      "swapUsedPercent": 0,
      "loadOne": 0.42
    }
  ]
}
```

TinyTop may include additional point fields for future chart series.
