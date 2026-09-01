# 0031 - A settings write configures the collector immediately, and the configure path reads its own settings

## Status

**Accepted (2026-09-01; 0.7.x)** — Michel's instruction, verbatim: *"please proceed with S1-S3"*, which
answers open question 2 of `docs/plans/2026-09-01-settings-correctness-plan.md` (D2 lives). Amends the
tick-ordering decision recorded in the T12 brief; supersedes nothing. Anchors verified at main `f573f7e`
(v0.7.1).

## Context

`collect_and_store` (`agent/crates/tinytop-agent/src/writer.rs:754-771`) runs, in order:

1. `collector.collect()` — `:755-758`
2. publish to `latest_snapshot` — `:759-761`
3. `store.insert_snapshot(...)` — `:762-766`
4. `store.get_settings()` — `:767`
5. `maintain_history(...)` — `:768`
6. `configure_collector_if_changed(state, &settings)` — `:769`

So a settings change is *read* at step 4 and *applied* at step 6, taking effect on the **next** tick.
Disabling thermals permits at most one further scan (≤ 1.5 s at the default poll); enabling waits a tick.
To an operator this reads as a switch that does not work.

This ordering is deliberate — the T12 brief fixed it as "configure AFTER publish → next-tick semantics",
and `configure_collector_if_changed` (`:792-803`) carries an explicit lock-order comment. Changing it is
therefore a design decision, not a bug fix.

**Two facts established at source before deciding** (both were assumptions in the plan):

- **The lock order is safe for a second caller.** The whole crate has exactly three non-test lock
  acquisitions: `collector` at `:756` (collection) and `:800` (configure), and `collector_config` at
  `:796` (configure). Neither `update_settings` (`:491-508`) nor `import_settings` (`:467-489`) holds
  either lock, so adding them as callers introduces no reverse acquisition path. The existing comment's
  justification — "collection takes only the collector lock" — still holds.

- **There is a stale write-back race that the naive nudge does not fix, and can make worse.** Steps 4
  and 6 are not atomic, and step 5 (`maintain_history`) prunes and folds, so the window between them is
  wide — not nanoseconds. If a settings write lands inside it:

  | | tick | write path |
  |---|---|---|
  | t0 | `get_settings()` → `S_old` | |
  | t1 | `maintain_history` (pruning) | writes `S_new`, configures collector → `C_new` |
  | t2 | `configure_collector_if_changed(&S_old)` → applies **`C_old`** | |

  The `applied == desired` guard does not save this: `applied` is `C_new`, `desired` is `C_old`, they
  differ, so the tick cheerfully reverts the operator's change for one tick. The setting appears to
  apply, silently reverts, then re-applies. That is a worse symptom than the lag being fixed.

## Decision

**1. Both settings write paths configure the collector after a successful write.** `update_settings`
(`:491`) and `import_settings` (`:467`) each call `configure_collector_if_changed` once the write has
committed and `maintain_history` has run. The plan named only `PUT /api/settings`; an import is equally
a settings write, and leaving it out would reintroduce the inconsistency D2 exists to remove.

**2. `configure_collector_if_changed` reads the settings itself, while holding `collector_config`.**
The signature loses its `&DashboardSettings` parameter and becomes
`async fn configure_collector_if_changed(state: &AppState)`. It acquires `collector_config`, *then*
calls `state.store.get_settings()`, then compares and applies.

This makes the `collector_config` mutex the single serialization point for "decide what the collector
should be, and make it so." Two racing callers no longer matter: whichever acquires the lock second
re-reads and sees the newer row, so the last writer to the *store* is always the last writer to the
*collector*. The race in Context is closed by construction rather than by ordering luck.

**3. The function stays infallible.** A failed settings read logs once and returns without configuring;
the next tick retries. It does not fail the HTTP request (the write already committed) and does not
abort startup.

**4. The tick loop is otherwise unchanged.** Steps 1–5 keep their order and their reasons. Step 6 keeps
its position; it simply no longer passes a value it read six lines earlier. `collect_and_store` still
reads settings at `:767` for `maintain_history`, which is a separate consumer.

## Alternatives rejected

1. **Reorder the tick to configure-then-collect** (the plan's option (a)). Immediate, but it inverts
   semantics T12 chose deliberately, moves work into a window kept clear on purpose, and forces every
   existing tick-ordering test to be re-reasoned. Cost out of proportion to the symptom.

2. **Nudge the collector with the settings the write path already has** (the plan's option (b) as
   written). This is the obvious implementation and it is the one that produces the revert-for-one-tick
   flicker above. Rejected on the evidence in Context: it converts a predictable 1.5 s lag into an
   intermittent, timing-dependent reversal — harder to diagnose than the defect it fixes.

3. **A settings generation counter** (`AtomicU64` in `AppState`, bumped on every write; ignore a
   `desired` whose generation is older than `applied`). Correct and avoids I/O under the lock, but to be
   sound the tick must sample the counter *before* its read and re-check after — a seqlock, hand-rolled,
   for a row that costs one indexed `SELECT` to re-read. More state and more subtlety than the problem
   earns.

4. **A settings mutex spanning the tick's read→configure span and the whole write path.** Simple to
   reason about and unacceptable in practice: it would hold a lock across `maintain_history`, so a
   prune on a multi-gigabyte database would block every settings write for its duration.

5. **Document the lag and drop D2** (offered to Michel as open question 2). He answered by ordering all
   three lanes.

## Consequences

- **One extra `get_settings()` per tick**, issued under `collector_config`. That mutex is acquired at
  exactly one site in the crate and held only for this function, so contention is nil; the read is a
  single-row indexed select. This is the price of the atomicity in decision 2 and should be stated in
  any future performance discussion rather than rediscovered.

- **A `tokio::sync::Mutex` is now held across an `.await`.** This is supported and intended for that
  type, and it is **not** a violation of the OTel status-guard invariant (`writer.rs:812-813`), which
  forbids holding *that* guard across a sleep, a pipeline shutdown, or a store call. The two locks are
  unrelated: `collector_config` is acquired at one site, guards no cross-task status read, and nothing
  the store does can depend on it. A reviewer who flags this should be pointed here.

- **Four production call sites** instead of two: startup (`:311`), the tick (`:769`), `update_settings`,
  `import_settings`. The startup call keeps its guarantee that the collector is configured before the
  first collection whenever settings are readable — pinned by the existing
  `collector_is_configured_from_settings_before_the_first_collection` test.

- **`configure_calls()` counts change for HTTP-driven paths only.** The existing
  `collect_and_store_reconfigures_only_when_the_settings_changed` test drives the store directly via
  `put_settings` rather than through the router, so its assertions are unaffected — verified before
  deciding. Any *new* test that saves through the router will see one configure at save time and none on
  the following tick, which is the whole point.

- **The idempotence guard remains load-bearing.** Because the write path and the tick can both call in,
  `applied == desired` is what keeps a settings save from reconfiguring the collector twice. It must not
  be removed as "redundant".
