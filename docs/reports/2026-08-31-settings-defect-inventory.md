# Settings defect inventory — 2026-08-31

**Status: evidence base, not a plan.** Written at Michel's "plan fixing settings for tinytop"
before the scope was settled, so that the verified findings survive a context rotation. Every
claim below was read at source on main `0000f80` (v0.6.0) or observed during T17 hardware
acceptance. A plan should be written against this, not against memory.

**Why it exists:** several of these were found by *hardware acceptance*, not by any test, and
would be expensive to rediscover.

---

## ⚠️ Live collision — read before touching anything dashboard-shaped

**T18 and T18b are running right now** (branches `tinytop/phase5-t18`, `tinytop/phase5-t18b`,
both based on `0000f80`). T18b rewrites `agent/assets/dashboard/{index.html,app.js,ladder-rules.js,styles.css}`
into the ADR 0027 tabbed shell. **Any dashboard-side settings fix must either wait for T18b to
merge or be folded into it** — otherwise it collides with an in-flight lane on the same files.

The backend/CLI defects below (**D1–D3**) are disjoint from both lanes and can proceed
independently.

---

## D1 — `collect --json` never reads persisted settings *(not covered by any lane)*

**Severity: the highest of these.** It is the only one that writes wrong data.

`collect()` at `agent/crates/tinytop-agent/src/main.rs:229` builds
`NativeCollector::default()` and never calls `store.get_settings()`. Verified: the `collect`
arm (`main.rs:210-243`) contains **zero** `get_settings` calls.

**The asymmetry is the real smell** — three sibling CLI paths *do* load settings:

| CLI path | loads settings? | site |
|---|---|---|
| `db stats` | yes | `main.rs:308` |
| `config export` | yes | `main.rs:445` |
| serve path | yes | `main.rs:801` |
| **`collect`** | **no** | `main.rs:210-243` |

**Consequences.** With thermals enabled, `tinytop-agent collect --json` reports no sensors
forever, so the CLI cannot be used to verify the feature. Worse, `collect --sqlite <db>`
**inserts sensor-less rows into a real database**, because the same default collector feeds
`insert_snapshot`.

**Why nothing caught it:** thermal is the *first settings-gated* collector — GPU is
detection-gated (on whenever the hardware is present), and every other knob is either
store-side or dashboard-side. There was no prior case where a CLI collector had to consult
settings, so no test encoded the expectation. It surfaced only when acceptance ran the real
binary on real hardware and got an empty `sensors` array with `thermal.enabled = true`
persisted.

**Documented** in `README.md` (Thermals section) and `PROGRESS.md` backlog as shipped
behaviour for 0.6.0.

**Design question a plan must answer:** should `collect` load settings *always*, or only when
`--sqlite` is given? Loading always makes the CLI agree with the daemon (good), but makes a
bare `collect --json` depend on a database that may not exist. The `--sqlite`-only rule keeps
`collect --json` hermetic but leaves the two modes inconsistent with each other.

## D2 — every settings change lands one collection late *(not covered by any lane)*

`collect_and_store` (`agent/crates/tinytop-agent/src/writer.rs:708`) runs in this order:

1. `collector.collect()` — **line ~710**
2. `state.store.get_settings()` — **line 721**
3. `maintain_history(...)` — line 722
4. `configure_collector_if_changed(...)` — **line 723**

So a settings change read at step 2 is applied at step 4 and only takes effect on the **next**
tick. Disabling thermals permits at most one further scan (≤ 1.5 s); enabling waits one tick;
`extraChips` settles at the next slow tick when discovery re-runs.

**This is deliberate**, not an accident: the T12 brief fixed the tick order as "configure AFTER
publish → next-tick semantics", and `configure_collector_if_changed` carries an explicit
lock-order comment (`collector_config -> collector`) justifying the arrangement. It is
therefore a **design change, not a bug fix**, and needs an ADR if changed.

**Cost:** a user toggles a setting and the UI disagrees with them for up to a tick, which reads
as a broken switch. Raised in luna's T17 review (MED, accepted as document-only) and now
documented in `README.md`.

**Design question:** reordering to configure-then-collect makes changes immediate but means a
tick can collect under settings the operator changed mid-tick, and it moves work inside the
window that the current order deliberately keeps out. Alternative: keep the order but have the
*write path* (`PUT /api/settings`) nudge the collector directly, so interactive changes feel
immediate while the tick loop stays as-is.

## D3 — backend and frontend validators disagree in wording *(not covered by any lane)*

Same three rules, two texts. Whichever validator fires first decides what the user sees.

| rule | backend (`tinytop-store/src/thermal_settings.rs`) | frontend (`ladder-rules.js`) |
|---|---|---|
| count | `thermal.extraChips accepts at most 16 chip names` | `thermal.extraChips must hold at most 16 entries` |
| duplicates | `thermal.extraChips contains duplicate chip name "x"` | `thermal.extraChips must not contain duplicates` |
| pattern | (hand-rolled char check) | `thermal.extraChips entries must match ^[a-z0-9_]{1,32}$` |

The **limits agree** (16 entries, 32 chars) — only the messages diverge. Frontend work here
collides with T18b; the backend half does not.

---

## Already covered — do NOT fix these separately

**C1 / C2 are F5 of the T18b brief, in flight.** From the T17b review
(`Fabulous/docs/fleet/tinytop/2026-08-31-t17b-fable-review.md`, MED-1):

- **C1** — `saveDaemonSettings` (`app.js:3156`) does
  `if (!response.ok) throw new Error(...HTTP ${response.status})` **without reading the response
  body**, so the server's explanation is discarded and the user sees a bare
  `Settings save failed with HTTP 400`.
- **C2** — the client validator does not mirror T17-fix1's reserved-chip rule
  (`amdgpu`/`i915`/`nvme`), which is the first server-side thermal rule with no client
  counterpart. Until C1 or C2 lands, typing `amdgpu` yields the bare 400 above.

## Minor, from the T17b review — defensive only

- **M1** — `usableSensorThreshold` checks `Number.isFinite(v) && v > 0`, mirroring only the
  lower half of ADR 0026 decision 4's `0 < v <= 200` band. Unreachable from our own backend
  (the collector filters before serialising), but a 65261 °C ceiling from any other producer
  would render as a permanently-empty bar.
- **M2** — `groupSensorsByChip` does `const chip = sensor?.chip` with no guard, so a reading
  without `chip` becomes a group headed by the string `undefined`. Unreachable from our own
  backend (chips with an empty/unreadable `name` are skipped).

---

## Suggested sequencing (for whoever writes the plan)

1. **D1 first** — it is the only defect that writes wrong data, and it is fully disjoint from
   both running lanes. Its design question is small and answerable.
2. **D3 backend half** — trivial, disjoint, and best done while D1 is open in the same crate.
3. **D2** — needs an ADR because it changes deliberate tick semantics. Worth doing, but it is a
   decision before it is a change.
4. **D3 frontend half + M1 + M2** — after T18b merges, or folded into it. Not before.
