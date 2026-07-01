# Nginx Subpath Integration Report

Date: 2026-06-30
Version: 0.1.35
Status: resolved in 0.2.1 (see "Resolution" below)

> **Resolution (0.2.1).** The recommended long-term fix below has shipped. The
> dashboard now uses document-relative asset URLs and derives its API base path
> from the document location, and both runtimes accept `--base-path` /
> `TINYTOP_BASE_PATH`. See ADR 0012 and the "Resolution" section at the end of
> this report. The nginx options below remain valid; Option 3's `sub_filter`
> rewriting is no longer necessary.

## Context

TinyTop ships the Rust collector/dashboard daemon as a single binary. The daemon
serves the embedded dashboard, static assets, telemetry APIs, and SQLite-backed
history APIs on one loopback listener.

Default Linux/WSL runtime:

```text
http://127.0.0.1:4274
```

The reported deployment shape was:

```text
https://<domain>/mon/ -> http://127.0.0.1:4274
```

The browser loaded the dashboard shell through nginx, but CSS and JavaScript
appeared missing.

## Root Cause

The release binary does embed the dashboard files. The integration issue is path
mounting, not missing files in the binary.

TinyTop currently serves the dashboard as a root-mounted application:

```text
/
/styles.css
/app.js
/vendor/echarts.min.js
/api/version
/api/settings
/api/snapshot
/api/history
/api/history/points
/api/history/markers
/api/history/coverage
```

The dashboard also references root-absolute URLs in the browser:

```html
<link rel="stylesheet" href="/styles.css" />
<script src="/vendor/echarts.min.js"></script>
<script src="/app.js" type="module"></script>
```

The JavaScript API calls also use root-absolute paths:

```text
/api/version
/api/settings
/api/snapshot
/api/history...
```

When the dashboard is mounted under `/mon/`, the browser still requests
`/styles.css`, `/app.js`, `/vendor/echarts.min.js`, and `/api/...` from the
domain root unless nginx or TinyTop rewrites those URLs. A `/mon/` proxy alone
therefore does not make the application base-path aware.

## Integration Options

### Option 1: Dedicated Hostname Or Subdomain

Recommended when available.

Example:

```text
https://mon.example.com/ -> http://127.0.0.1:4274
```

Nginx:

```nginx
server {
    server_name mon.example.com;

    location / {
        proxy_pass http://127.0.0.1:4274;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

Why this is safest:

- TinyTop's existing root-absolute asset and API URLs work unchanged.
- No response rewriting is needed.
- It avoids collisions with an existing application at the domain root.

### Option 2: Subpath With Root Routes Also Proxied

Use this only if TinyTop can own the root static and API paths on the same
domain.

```nginx
location = /mon {
    return 301 /mon/;
}

location /mon/ {
    proxy_pass http://127.0.0.1:4274/;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_set_header X-Forwarded-Prefix /mon;
}

location = /styles.css {
    proxy_pass http://127.0.0.1:4274/styles.css;
}

location = /app.js {
    proxy_pass http://127.0.0.1:4274/app.js;
}

location /vendor/ {
    proxy_pass http://127.0.0.1:4274/vendor/;
}

location /api/ {
    proxy_pass http://127.0.0.1:4274/api/;
}
```

Trade-off:

- This works with the current dashboard.
- It is not safe if the domain root already has its own `/api/`, `/app.js`,
  `/styles.css`, or `/vendor/` routes.

### Option 3: Subpath With Nginx HTML/JS Rewriting

Use this when the domain root belongs to another app and TinyTop must remain
under `/mon/`.

```nginx
location = /mon {
    return 301 /mon/;
}

location /mon/ {
    proxy_pass http://127.0.0.1:4274/;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_set_header X-Forwarded-Prefix /mon;

    sub_filter_once off;
    sub_filter_types text/html application/javascript text/javascript;
    sub_filter 'href="/styles.css"' 'href="/mon/styles.css"';
    sub_filter 'src="/vendor/echarts.min.js"' 'src="/mon/vendor/echarts.min.js"';
    sub_filter 'src="/app.js"' 'src="/mon/app.js"';
    sub_filter 'fetch("/api/' 'fetch("/mon/api/';
    sub_filter 'fetch(`/api/' 'fetch(`/mon/api/';
}
```

Trade-off:

- This preserves the `/mon/` public mount without stealing root routes.
- It is brittle because dashboard source changes can introduce new absolute
  paths that nginx does not rewrite.
- It should be treated as an operational workaround, not the ideal long-term
  contract.

## Recommended Long-Term Product Fix

TinyTop should grow first-class base-path support.

Suggested contract:

```bash
tinytop-agent serve --base-path /mon
```

or:

```bash
TINYTOP_BASE_PATH=/mon tinytop-agent serve
```

Expected behavior:

- `/mon/` serves the dashboard shell.
- `/mon/styles.css`, `/mon/app.js`, and `/mon/vendor/echarts.min.js` serve
  embedded static assets.
- `/mon/api/...` serves the same API handlers as `/api/...`.
- The dashboard uses a generated or runtime-discovered base path for all asset
  and API URLs.
- `/api/version` can continue to exist for backwards-compatible root-mounted
  deployments unless explicitly disabled.

This avoids nginx response rewriting and makes reverse-proxy deployments
predictable.

## Verification Checklist

For a root-mounted deployment or dedicated subdomain:

```bash
curl -fsS https://mon.example.com/api/version
curl -I https://mon.example.com/styles.css
curl -I https://mon.example.com/app.js
curl -I https://mon.example.com/vendor/echarts.min.js
```

For a `/mon/` deployment:

```bash
curl -fsS https://example.com/mon/api/version
curl -I https://example.com/mon/styles.css
curl -I https://example.com/mon/app.js
curl -I https://example.com/mon/vendor/echarts.min.js
```

Expected `/api/version` signal:

```json
{
  "runtime": "rust",
  "component": "collector-dashboard-daemon",
  "dashboard": "embedded"
}
```

If `/api/version` reports `dashboard: embedded` but CSS or JS still fails
through nginx, the binary is not missing files; the proxy path mapping is wrong.

## Current Conclusion

The `v0.1.35` release binary embeds the dashboard assets correctly. The nginx
subpath integration failed because TinyTop is currently root-mounted and the
dashboard uses root-absolute asset and API URLs. The best immediate fix is a
dedicated hostname or root-route proxying. The best durable fix is explicit
TinyTop base-path support.

## Resolution (shipped in 0.2.1)

Both durable fixes recommended above are now implemented (ADR 0012):

- The dashboard shell references assets with **document-relative** URLs
  (`styles.css`, `app.js`, `vendor/echarts.min.js`, `favicon.svg`) and derives
  its API prefix from the current document location. It loads correctly at `/`,
  at `/embed`, and under any subpath served with a trailing slash — no nginx
  response rewriting required.
- The daemon accepts an explicit mount prefix:

  ```bash
  tinytop-agent serve --base-path /mon
  # or
  TINYTOP_BASE_PATH=/mon tinytop-agent serve
  ```

  The Bun runtime honors `TINYTOP_BASE_PATH` as well. With a base path set the
  daemon serves the dashboard, assets, and APIs under `/mon/...`, redirects the
  bare `/mon` to `/mon/`, and keeps the root routes live for backwards-compatible
  deployments.

### Recommended nginx for a subpath now

Because the dashboard is base-path relative, a prefix-stripping proxy is enough
and no `sub_filter` is needed:

```nginx
location = /mon {
    return 301 /mon/;
}

location /mon/ {
    proxy_pass http://127.0.0.1:4274/;   # trailing slash strips /mon/
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

If you prefer to forward the prefix unchanged (no trailing slash on
`proxy_pass`), run the daemon with `--base-path /mon` so it strips the prefix
itself.
