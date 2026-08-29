# ADR 0020 — The hourly disk check keeps the last known state when free space is undeterminable; no hysteresis; state + marker in one transaction; first check at daemon start

**Status:** Accepted (2026-08-29) — decided under the tinytop-ladder GO for Phase 2 Task 9; builds on 0017 (the measurement) and the spec's §9 hourly block.

## Context

Spec §9 defines the hourly disk check in one paragraph: measure free bytes on the database's mount, `free < minFreeBytes` ⇒ `history_state.diskPressure = {active:true,…}` + marker `diskPressure` once per breach; recovery ⇒ `{active:false}` + marker `diskRecovered`; pressure never deletes, it only refuses growth (§5) and shows a banner. It is silent on four things a lane would otherwise guess:

1. **What happens when the measurement fails.** ADR 0017 made the *migration's* free-space guard fail closed ("undeterminable is a refusal, not a skip") because that guard protects a one-shot pre-image write. The hourly check is a standing state that gates every settings save for as long as it is active.
2. **Whether recovery has hysteresis.** A box hovering at the threshold would flip every interval, writing a `diskPressure`/`diskRecovered` pair each time.
3. **Whether the state, `lastDiskCheckMs` and the marker are one write or three.** They are three different rows (`history_state` ×2, `app_events`).
4. **When the first check runs.** The daemon may boot onto a disk that changed while it was down (the operator freed space precisely so it would start); stale state from the previous run would refuse growth until the first interval elapsed.

## Decision

1. **Undeterminable ⇒ keep the last known state.** When the provider returns an `io::Error` (no mount contains the path, the path cannot be canonicalised, `statvfs` failed), the check writes **nothing** — `diskPressure` and `lastDiskCheckMs` are untouched, no marker — and returns `StoreError::DiskCheck { path, source }`, which the daemon logs at error every interval. An active breach stays active (a broken measurement never lifts a real breach); an inactive state stays inactive (a broken measurement never manufactures a breach whose message could not even name `free X`). The stale `lastCheckMs` in coverage and `db stats` is the honest signal that the check is not running.
2. **No hysteresis.** `breach = free < minFreeBytes`, recovery = the complement, exactly as §9 writes it. A flapping box writes at most one marker pair per interval (≥ 5 minutes); that is visible, bounded, and truthful. A recovery margin can be added later as a settings field without a schema change; inventing one now would make the banner disagree with the setting the operator typed.
3. **One transaction.** The state row, `lastDiskCheckMs` and the marker (when a transition happened) commit together (`record_event_on` split out of `record_event`, the same pattern as `history_state_set_on`). A kill between them cannot leave an active state without its marker, a marker without its state, or a `lastCheckMs` that claims a check whose state was lost.
4. **First check immediately at spawn, then every `intervalMinutes`** as read from the settings on that iteration (a changed interval applies at the next tick). The measurement runs on a blocking thread (`spawn_blocking`) because `sysinfo` performs a `statvfs` per mount and a hung network mount must not stall the HTTP runtime.

## Alternatives rejected

- **Fail closed (activate pressure on an undeterminable measurement), mirroring 0017.** Rejected: 0017 protects a single write that can wait; this would refuse every horizon growth, tier enable and archive enable on any box whose mount `sysinfo` cannot see (containers with unusual mounts, some Windows volumes), with a refusal message that cannot state the free bytes it is refusing on. The cost is standing, not one-shot.
- **Fail open (clear pressure on an undeterminable measurement).** Rejected: a real breach must never be lifted by a measurement failure.
- **Hysteresis (e.g. recover only at 1.1 × `minFreeBytes`).** Rejected for now: not in the spec, and a hidden margin makes the banner contradict the configured threshold. Revisit as an explicit setting if marker churn is observed in production.
- **Three independent writes.** Rejected: cheaper by one `BEGIN`, and it creates exactly the split-brain states listed above.
- **First check after the first interval (like the cold export's 60-second grace).** Rejected: the cold export is heavy and idempotent; the disk check is milliseconds and its stale state actively refuses operator actions.

## Consequences

- A box that cannot be measured shows a red-less, stale coverage (`lastCheckMs` frozen) and an error line per interval in the daemon log; nothing is refused because of the failure itself.
- Marker volume under flapping is bounded by the interval floor (5 minutes).
- `DiskPressureState` gains `sinceMs` (spec §6 already lists it) and becomes serialisable; every existing document parses unchanged.
- `StoreError` gains a `DiskCheck` variant; §15's rule (every error names field, rule, observed value, remedy) holds — the message names the path, the failure, and the two remedies (fix the mount, or lower `retentionLadder.diskCheck.minFreeBytes`).
