# Plan — settings correctness (D1–D3)

**Status: GO (Michel, 2026-09-01, verbatim: *"please proceed with S1-S3"*).**
**Author:** fable@fabulous, 2026-09-01. Evidence base: `docs/reports/2026-08-31-settings-defect-inventory.md`
(every file:line in the ORIGINAL text below was verified there at main `0000f80`).

> ## ⚠️ AMENDMENT — 2026-09-01, at the GO, re-verified at main `f573f7e` (v0.7.1)
>
> **The body of this plan below is preserved as written, but parts of it are now WRONG.** It was
> authored at `0000f80`; main has since moved through T18, T18b, 0.7.0 and 0.7.1, which touched the
> settings surface directly. **The briefs in `2026-09-01-settings-correctness/briefs/` are authoritative
> — a lane must trust them, not this document.** What changed:
>
> **His three answers.** (1) *Scope* — all three lanes ordered, so this is the **defect** reading; the
> settings *experience* was largely addressed separately in 0.7.0/0.7.1 (ADRs 0027–0030). (2) *D2 lives.*
> (3) *Release shape* — **mine, per the repo's own convention**: per-lane patch tags as each merges
> (0.7.2, 0.7.3, 0.7.4 in merge order), then the set closes at **0.8.0** with a `gh release` and audits.
> S2 changes deliberate behaviour, which is what earns the minor.
>
> **Anchors moved.** `writer.rs` shifted ~46 lines: `collector_config_from` `:727`→**`:773`**,
> `collect_and_store` `:708`→**`:754`**, `changed_keys` `:451`→**`:497`**. `main.rs` anchors
> (`:229`, `:210-243`, `:308/445/801`) are unchanged.
>
> **The gate baseline in §"Gate requirements" is wrong.** It says 385; the measured baseline at
> `f573f7e` is **28 suites / 411 passed / 0 failed / 2 ignored** (Bun **261 / 0** across 22 files),
> run by Fable immediately before dispatch. 385 never reconciled against its own tree. A lane briefed
> with 385 would report a false green.
>
> **D3 no longer has a backend half.** The pattern rule this plan and the inventory describe as a
> "hand-rolled char check" now carries the *identical* canonical string on both sides
> (`thermal_settings.rs:41` == `ladder-rules.js:363`). Separately, the inventory's **C2** (client
> missing the reserved-chip rule) was **fixed by T18b** — which turned a missing rule into a fourth
> wording divergence. Net: **D3 is entirely frontend**, so S1 becomes pure Rust and S3 pure JS, and
> the two are cleanly disjoint and run in parallel.
>
> **D3 is not only wording — the evaluation ORDER differs.** The backend validates per element with
> early return (`thermal_settings.rs:33-55`); the client validates per rule across the whole array
> (`ladder-rules.js:372-379`). For `["cpu_a","cpu_a","amdgpu"]` the server reports *duplicate* and the
> client reports *reserved* — **a different rule fires.** Copying the strings alone would leave this in
> place and look fixed. S3 adopts the backend's per-element order.
>
> **D2's design is superseded by ADR 0031.** The plan's recommended option (b) — nudge the collector
> with the settings the write path already holds — **introduces a stale write-back race**: the tick
> reads settings and applies them with `maintain_history` (a prune) in between, so a write landing in
> that window is reverted for one tick, and `applied == desired` cannot detect it. ADR 0031 instead
> removes the settings parameter and has the configure path read the row itself under the
> `collector_config` guard. The plan's open risk (*"the handler does not hold the collector lock — that
> must be verified at source"*) **is now verified**: the crate has exactly three non-test lock
> acquisitions (`writer.rs:756`, `:796`, `:800`) and neither handler holds either. ADR 0031 also adds
> **`POST /api/settings/import`** as a second write path, which this plan omitted.
>
> **D1's headline test cannot be written as specified.** `NativeCollector::thermal_root`
> (`tinytop-collectors/src/linux.rs:122`) is a private field with no setter and no env override, and
> `collect()` lives in a different crate — so *"fixture hwmon root → non-empty `sensors`"* is
> unreachable from `tinytop-agent` without making thermals injectable, which this plan explicitly
> defers. The CI proof is `topProcessCount` instead (identical single `configure` call, observable on
> any host); the thermal-specific end stays with Fable's hardware acceptance, where it was found.

---

## The assumption I am making, stated so you can kill it cheaply

I asked whether *"fixing settings"* meant the **backend/CLI correctness defects** or the
**settings experience**, and planned before the answer arrived. **This plan takes the first
reading:** settings that do not do what the operator told them to do.

**If you meant the experience** — the dialog being annoying, the wrong things being prominent,
too many knobs — **stop here and say so.** That is a different plan, it is mostly T18b's
surface, and it should be designed against what actually irritates you rather than against my
guess. Nothing below is wasted in that case; D1 stands on its own merits regardless.

**Explicitly OUT of scope**, because T18b already owns them (its F5) and double-fixing would
collide: the save path discarding the server's error body, and the client validator not
mirroring the reserved-chip rule.

## Why this is worth doing at all

One of these writes wrong data (**D1**), one makes a switch look broken (**D2**), and one makes
the error you see depend on which validator fired (**D3**). D1 alone justifies the work: with
thermals enabled, `tinytop-agent collect --sqlite <db>` inserts sensor-less rows into a real
database, silently and forever.

---

## D1 — `collect` must honour the settings of the database it writes to

**Defect.** `collect()` (`main.rs:210-243`) builds `NativeCollector::default()` at
`main.rs:229` and never calls `get_settings()`. Three sibling CLI paths do (`db stats` :308,
`config export` :445, serve :801). Thermal is the first *settings-gated* collector — GPU is
detection-gated — so no prior test encoded the expectation, and hardware acceptance found it.

**The design question, and my answer.** Should `collect` always load settings, or only with
`--sqlite`?

**Only with `--sqlite`**, and the rule that makes it coherent is:
**the collector is configured from the settings stored in the database the rows are going into.**

- `collect --json` (no database) stays hermetic and unchanged — it collects with defaults, has
  no database to consult, and must not invent one. Making it read `default_sqlite_url()` would
  have a bare debug command touch (and potentially create) the live database. Unacceptable.
- `collect --json --sqlite <db>` loads that database's settings and configures the collector
  before collecting, so what is written matches what that database says it wants.

This is explainable in one sentence and removes the surprise without adding a mode.

**Shape of the change** (all inside `tinytop-agent`, which is why it is small):

1. `writer.rs:727` — `fn collector_config_from` becomes `pub(crate) fn`. One word; `writer` is
   already `mod writer;` of the same crate (`main.rs:22`).
2. `main.rs` `collect()` — when `sqlite_url` is `Some`, open the store *before* collecting,
   `get_settings()`, and `collector.configure(collector_config_from(&settings))`.
   `CollectorConfig` already carries exactly the four fields needed
   (`top_process_count`, `filesystems_interval`, `thermal_enabled`, `thermal_extra_chips`) and
   `configure` is already on the `Collector` trait.
3. Ordering note: the store is currently opened *after* collection. It must move before, which
   also means a store-open failure now fails the command before any collection — correct, and
   it must be an explicit error, never a silent fall back to defaults.

**Tests (RED first).**
- `collect_with_sqlite_honours_persisted_thermal_settings` — a fixture DB with
  `thermal.enabled = true` and a fixture hwmon root produces a snapshot whose `sensors` is
  non-empty, and the inserted rows are non-zero.
- `collect_without_sqlite_uses_defaults` — no database argument, no settings read, behaviour
  byte-identical to today.
- `collect_with_sqlite_honours_top_process_count` — proves the fix is general, not thermal-only.
- A store-open failure surfaces as an error, not as default settings.

**Docs.** `README.md`'s Thermals section currently documents the *defect* as shipped behaviour
— that paragraph must be rewritten, not merely amended. `docs/guides/API.md` CLI section too.

## D2 — a settings change should feel immediate *(needs an ADR before any code)*

**Defect.** `collect_and_store` (`writer.rs:708`) runs collect (~710) → `get_settings()` (721)
→ `configure_collector_if_changed()` (723), so a change applies on the **next** tick: disabling
thermals permits one more scan (≤ 1.5 s), enabling waits a tick.

**This is deliberate.** The T12 brief fixed the order as "configure AFTER publish → next-tick
semantics", and `configure_collector_if_changed` carries an explicit lock-order comment
(`collector_config -> collector`). **It is therefore a design change, not a bug fix, and it
gets an ADR at decision time — not a lane.**

**Two approaches; I recommend the second.**

- **(a) Reorder to configure-then-collect.** Immediate, but it inverts semantics the T12 brief
  chose on purpose, and moves work inside a window deliberately kept clear. Every existing
  tick-ordering test has to be re-reasoned. Cost is out of proportion to the symptom.
- **(b) Leave the tick loop alone; have the write path nudge the collector.** ✅ **Recommended.**
  `PUT /api/settings` already computes `changed_keys` (`writer.rs:451`), so the hook point
  exists. After a successful write, call the same `configure_collector_if_changed` the tick
  calls. Interactive changes take effect at once; the periodic loop keeps its proven
  invariants; the change is additive.

  **The risk that makes this architectural, and must be discharged in the ADR:** it introduces
  a *second caller* of the configure path, so the `collector_config -> collector` lock order
  must be proven to hold from the HTTP handler, which runs concurrently with the tick. The
  handler does not hold the collector lock today — that must be verified at source, not
  assumed, and a test should exercise a PUT racing a tick.

**Do not dispatch D2 until the ADR is written and accepted.**

## D3 — one message per rule

Backend (`thermal_settings.rs`) and frontend (`ladder-rules.js`) word the same three rules
differently; the *limits agree* (16 entries, 32 chars), only the text diverges.

**The backend is authoritative** — it is the one that can actually refuse — so the frontend
adopts the backend's exact strings.

**Split by collision:** the backend half is disjoint and can go with D1. **The frontend half
touches `ladder-rules.js`, which T18b rewrites, so it waits for T18b to merge** and should then
be folded in alongside the inventory's M1/M2 (the half-mirrored threshold band; the `undefined`
chip group).

---

## Lanes, sequencing, and tiering

| # | Lane | Scope | Model | Depends on |
|---|---|---|---|---|
| S1 | `settings-s1-collect-honours-settings` | D1 + D3 backend half | `ari-sol-deep` (Rust) | nothing — disjoint from T18/T18b |
| — | ADR 00xx | D2's decision, written by me | — | — |
| S2 | `settings-s2-immediate-apply` | D2 approach (b) | `ari-sol-deep` (concurrency) | the ADR being accepted |
| S3 | `settings-s3-client-alignment` | D3 frontend + M1 + M2 | `ari-sol` (JS) | **T18b merged** |

**S1 can start immediately.** S3 cannot start before T18b merges. S2 is gated on a decision,
not on code.

`ari-sol-deep` for S1 and S2 per the standing tiering rule: Rust, and S2 touches a
concurrency/lock-order question — exactly the class that skips the low tier.

## Gate requirements — must be satisfiable INSIDE containment

Learned the hard way today: **T18b escalated because my brief demanded `bun run check:bun`,
which starts a server, and a hexe lane cannot bind a port.** Therefore, for every lane above:

- Rust lanes: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, and
  the per-crate `cargo test` invocations. **Full-workspace baseline to beat: 28 suites / 385
  passed / 0 failed** — and note that **385 contains 10 duplicate tests** from T17-fix1's
  `include!`, so a drop to 375 after any change touching that test is *not* a regression.
- JS lane: **`bun test` only.** Do **not** require `bun run check:bun`. There is deliberately
  no dashboard lint script; do not add one.
- **Nothing that binds a socket, starts a daemon, reaches the network, or touches
  `~/.local/share/tinytop/`.** Those are the orchestrator's acceptance steps, mine, outside the
  sandbox.

## Acceptance (mine, outside the sandbox)

- **D1 on real hardware:** on sheep, a scratch DB with `thermal.enabled = true`, then
  `collect --json --sqlite <scratch>` must report the 5 `coretemp` readings — the exact command
  that returned empty during 0.6.0 acceptance and exposed the defect.
- **D1 hermeticity:** bare `collect --json` still reads no database (prove with `strace`: no
  open of any `.sqlite`).
- **D2:** toggle thermals through the dashboard and observe the next snapshot, not the one
  after.
- Live `:4274` untouched throughout; any scratch daemon stopped at close-out.

## What I have deliberately NOT planned

- Anything in T18b's files before it merges.
- Any change to the `include!` test debt — that is its own task (it needs a decision about
  making `tinytop-collectors`'s `thermal` module public API to serve a test).
- Any settings *UX* change. That awaits your answer on scope.

## Open questions for Michel

1. **Scope** — the assumption at the top. Defects, or experience?
2. **D2 at all?** It is the only item that changes deliberate behaviour. If the one-tick lag
   has never actually bothered you, the honest move is to document it and drop it rather than
   spend an ADR and a concurrency-sensitive lane on a symptom nobody feels.
3. **Release shape** — S1 alone as a patch (0.6.1), or hold all three for one minor (0.7.0)
   alongside T18/T18b?
