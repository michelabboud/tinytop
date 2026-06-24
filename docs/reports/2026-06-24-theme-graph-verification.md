# Theme and Graph Verification

## Scope

Verifies the `0.1.1` display-control enhancement: selectable themes, selectable graph modes, persisted browser preferences, and responsive rendering.

## Expected Checks

- Theme controls are visible and keyboard-focusable native buttons.
- Graph mode controls are visible and keyboard-focusable native buttons.
- Selecting Solar changes `body[data-theme]` to `solar`.
- Selecting Heatmap changes the active graph mode and updates the visible history mode label.
- Theme and graph selections persist after reload.
- Desktop and mobile layouts do not horizontally overflow.

## Results

```bash
bun test
```

Result: 14 passed, 0 failed.

```bash
bun run src/server.ts --check
```

Result: live snapshot returned `status: ok` and runtime `WSL`.

```bash
curl -fsS http://127.0.0.1:4274/health
curl -fsS http://127.0.0.1:4274/api/snapshot
```

Result: health returned `ok`; snapshot returned runtime `WSL`, high confidence, CPU, memory, filesystem count, and process count.

```bash
bunx playwright test theme-graphs.qa.spec.js theme-graphs-full.qa.spec.js --reporter=line
```

Result: 3 passed, 0 failed.

## Visual QA

- Desktop viewport `1440x1000`: verified all five themes and all four graph modes. Screenshot inspected with Ember theme and Heatmap mode.
- Mobile viewport `390x900`: verified controls remain visible and there is no horizontal overflow.
