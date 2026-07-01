# 0012 - Reverse-Proxy Base-Path Support

## Status

Accepted

## Context

TinyTop serves the dashboard, static assets, and APIs as a root-mounted
application on `127.0.0.1:4274`. The dashboard shell referenced its assets with
root-absolute URLs (`/styles.css`, `/app.js`, `/vendor/echarts.min.js`,
`/favicon.svg`) and the browser called APIs at root-absolute paths (`/api/...`).

Operators reported that placing TinyTop behind nginx at a subpath
(`https://<domain>/mon/ -> http://127.0.0.1:4274`) loaded the HTML shell but
left CSS and JavaScript missing: the browser resolved the absolute URLs against
the domain root, not `/mon/`. `docs/reports/2026-06-30-nginx-subpath-integration.md`
diagnosed this and recommended two things — make the dashboard base-path aware,
and give the daemon first-class base-path support — instead of relying on
brittle nginx `sub_filter` HTML/JS rewriting.

## Decision

Two complementary changes:

1. **The dashboard is base-path relative.** `index.html` references its assets
   with document-relative URLs (`styles.css`, `app.js`, `vendor/echarts.min.js`,
   `favicon.svg`), and `app.js` derives its API prefix from the current document
   location (`dashboardBasePath()` → `DASHBOARD_BASE_PATH`). The shell therefore
   works unchanged at `/`, at `/embed`, and under any proxy subpath as long as
   the mount is served with a trailing slash. This alone fixes the reported
   deployment with a simple `proxy_pass` that strips the prefix, with no
   response rewriting.

2. **The daemon accepts an explicit mount prefix.** `tinytop-agent serve
   --base-path /mon` (or `TINYTOP_BASE_PATH=/mon`) and the Bun server's
   `TINYTOP_BASE_PATH` make the daemon serve the dashboard, assets, and APIs
   under `{base}/...`, redirect the bare `{base}` to `{base}/`, and keep the
   root routes live for backwards compatibility. This covers proxies that
   forward the prefix unchanged rather than stripping it.

In the Rust daemon, the prefix is stripped *before* routing by delegating to the
route table through an outer router fallback wrapped in a `from_fn` middleware,
because `Router::layer` runs after route matching. Static handlers read the
(rewritten) request `Uri` rather than `OriginalUri` so assets resolve under the
mount.

## Alternatives Rejected

### Nginx `sub_filter` HTML/JS rewriting

Rejected as the durable answer (kept only as an operational workaround in the
report). It is brittle: any new absolute path introduced in the dashboard
source silently breaks unless the nginx rules are updated in lockstep.

### Server-side HTML injection of a `<base>` tag or prefix

Rejected because it would make the served HTML diverge from the on-disk asset
trees and complicate the byte-identical dashboard invariant (ADR 0006).
Document-relative URLs achieve the same result with static assets.

### Axum `Router::nest` for the prefix

Rejected because nesting a router that has a root (`/`) route interacts awkwardly
with the bare-mount redirect and leaves `OriginalUri` as the full path, which the
static handlers depended on. A pre-routing URI rewrite is simpler and keeps the
single route table.

## Consequences

- TinyTop deploys behind a reverse-proxy subpath without HTML/JS rewriting.
- Root-mounted deployments are unchanged; `/api/version` and root assets keep
  working, so the change is backwards compatible.
- The mount must be served with a trailing slash; the daemon issues a permanent
  redirect from the bare prefix, and operators who strip the prefix in the proxy
  should add the same `{base}` → `{base}/` redirect.
- Dashboard asset changes still update both `legacy/dashboard/` and
  `agent/assets/dashboard/` identically (ADR 0006); the relative-URL contract is
  covered by `tests/dashboard-assets.test.ts`.
