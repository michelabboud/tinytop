use crate::{
    DashboardSettings, SqliteHistoryStore, StoreError,
    archive::{ARCHIVE_BATCH_ROWS, MAX_ARCHIVE_BATCHES_PER_TICK, archive_paths, move_expired_l4},
    ladder::{Tier, TierBucket, bucket_start_for, fold, grace_ms, is_complete},
    retention_ladder::{ArchiveSettings, RetentionLadder},
};

const MAX_PROMOTIONS_PER_TICK: i64 = 50;
const JSON_STRIP_BATCH: i64 = 500;
const ORPHAN_COMMAND_PRUNE_BATCH: i64 = 1_000;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LadderConfig {
    pub l1_keep_ms: i64,
    pub l2_keep_ms: i64,
    pub l3: Option<i64>,
    pub l4: Option<i64>,
    pub snapshot_json_keep_ms: i64,
    pub detail_interval_ms: i64,
    pub process_fast_keep_ms: i64,
    pub poll_interval_ms: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaintenanceReport {
    pub promoted_l3: i64,
    pub promoted_l4: i64,
    pub json_stripped: i64,
    pub pruned: [i64; 4],
    pub detail_rows: i64,
    pub detail_rows_pruned: u64,
    pub process_fast_rows: u64,
    pub orphan_commands: u64,
    pub expired_l4: i64,
    pub archived_l4: i64,
}

#[derive(Debug)]
pub struct MaintenanceError {
    pub report: MaintenanceReport,
    pub error: StoreError,
}

impl std::fmt::Display for MaintenanceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(formatter)
    }
}

impl std::error::Error for MaintenanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl From<StoreError> for MaintenanceError {
    fn from(error: StoreError) -> Self {
        Self {
            report: MaintenanceReport::default(),
            error,
        }
    }
}

// StoreError boxing is a Phase-3 refactor.
#[allow(clippy::result_large_err)]
pub async fn maintain(
    store: &SqliteHistoryStore,
    settings: &DashboardSettings,
    now_ms: i64,
) -> Result<MaintenanceReport, MaintenanceError> {
    settings.validate()?;
    maintain_with_archive(
        store,
        &settings
            .retention_ladder
            .to_ladder_config(settings.poll_interval_ms),
        &settings.retention_ladder.archive,
        now_ms,
    )
    .await
}

#[doc(hidden)]
// StoreError boxing is a Phase-3 refactor.
#[allow(clippy::result_large_err)]
pub async fn maintain_with_config(
    store: &SqliteHistoryStore,
    config: &LadderConfig,
    now_ms: i64,
) -> Result<MaintenanceReport, MaintenanceError> {
    maintain_with_archive(store, config, &ArchiveSettings::default(), now_ms).await
}

// StoreError boxing is a Phase-3 refactor.
#[allow(clippy::result_large_err)]
async fn maintain_with_archive(
    store: &SqliteHistoryStore,
    config: &LadderConfig,
    archive: &ArchiveSettings,
    now_ms: i64,
) -> Result<MaintenanceReport, MaintenanceError> {
    let mut report = MaintenanceReport::default();
    let mut first_error = None;

    match store.history_state_get::<i64>("pendingDetailRows").await {
        Ok(Some(count)) if count != 0 => {
            report.detail_rows = count;
            if let Err(error) = store
                .history_state_set("pendingDetailRows", &0_i64, now_ms)
                .await
            {
                record_step_error(&mut first_error, "clear pendingDetailRows", error);
            }
        }
        Ok(Some(_)) | Ok(None) => {}
        Err(error) => record_step_error(&mut first_error, "read pendingDetailRows", error),
    }

    for (key, enabled) in [
        ("l3Enabled", config.l3.is_some()),
        ("l4Enabled", config.l4.is_some()),
    ] {
        match store.history_state_get::<bool>(key).await {
            Ok(Some(current)) if current == enabled => {}
            Ok(_) => {
                if let Err(error) = store.history_state_set(key, &enabled, now_ms).await {
                    record_step_error(&mut first_error, &format!("persist {key}"), error);
                }
            }
            Err(error) => {
                record_step_error(&mut first_error, &format!("read {key}"), error);
            }
        }
    }

    // Pruning uses the watermarks visible at tick start. A promotion completed
    // in this tick therefore becomes deletion authority on the next tick.
    let initial_l3_watermark = read_watermark(store, "l3FoldedUntilMs", &mut first_error).await;
    let initial_l4_watermark = read_watermark(store, "l4FoldedUntilMs", &mut first_error).await;
    let grace = grace_ms(config.poll_interval_ms);

    if config.l3.is_some() {
        match promote(
            store,
            Tier::L2,
            Tier::L3,
            "l3FoldedUntilMs",
            now_ms,
            grace,
            None,
        )
        .await
        {
            Ok(count) => report.promoted_l3 = count,
            Err(error) => record_step_error(&mut first_error, "promote L3", error),
        }
    }

    if config.l4.is_some() {
        let source_tier = if config.l3.is_some() {
            Tier::L3
        } else {
            Tier::L2
        };
        let source_folded_until_ms = if source_tier == Tier::L3 {
            match store.history_state_get::<i64>("l3FoldedUntilMs").await {
                Ok(value) => value,
                Err(error) => {
                    record_step_error(&mut first_error, "read L3 watermark for L4", error);
                    None
                }
            }
        } else {
            None
        };
        match promote(
            store,
            source_tier,
            Tier::L4,
            "l4FoldedUntilMs",
            now_ms,
            grace,
            source_folded_until_ms,
        )
        .await
        {
            Ok(count) => report.promoted_l4 = count,
            Err(error) => record_step_error(&mut first_error, "promote L4", error),
        }
    }

    match store
        .strip_snapshot_json(
            now_ms.saturating_sub(config.snapshot_json_keep_ms),
            JSON_STRIP_BATCH,
        )
        .await
    {
        Ok(count) => report.json_stripped = to_i64_count(count),
        Err(error) => record_step_error(&mut first_error, "strip snapshot JSON", error),
    }

    match store
        .prune_raw_history(now_ms.saturating_sub(config.l1_keep_ms))
        .await
    {
        Ok(count) => report.pruned[0] = to_i64_count(count),
        Err(error) => record_step_error(&mut first_error, "prune L1", error),
    }

    match store
        .prune_detail_history(now_ms.saturating_sub(config.l2_keep_ms))
        .await
    {
        Ok(count) => {
            report.detail_rows_pruned = count;
            if count > 0 {
                eprintln!("history maintenance info: deleted {count} expired detail rows");
            }
        }
        Err(error) => record_step_error(&mut first_error, "prune detail rows", error),
    }

    match store
        .prune_process_fast_history(now_ms.saturating_sub(config.process_fast_keep_ms))
        .await
    {
        Ok(count) => {
            report.process_fast_rows = count;
            if count > 0 {
                eprintln!("history maintenance info: deleted {count} expired fast process rows");
            }
        }
        Err(error) => record_step_error(&mut first_error, "prune fast process rows", error),
    }

    // A command can only become orphaned when process rows are deleted.
    if report
        .detail_rows_pruned
        .saturating_add(report.process_fast_rows)
        > 0
    {
        loop {
            match store
                .prune_orphan_commands(ORPHAN_COMMAND_PRUNE_BATCH)
                .await
            {
                Ok(count) => {
                    report.orphan_commands = report.orphan_commands.saturating_add(count);
                    if count < ORPHAN_COMMAND_PRUNE_BATCH as u64 {
                        break;
                    }
                }
                Err(error) => {
                    record_step_error(&mut first_error, "prune orphan commands", error);
                    break;
                }
            }
        }
        if report.orphan_commands > 0 {
            eprintln!(
                "history maintenance info: deleted {} orphaned process commands",
                report.orphan_commands
            );
        }
    }

    let l2_dependent_watermark = if config.l3.is_some() {
        required_watermark(initial_l3_watermark)
    } else if config.l4.is_some() {
        required_watermark(initial_l4_watermark)
    } else {
        i64::MAX
    };
    let l2_cutoff = now_ms
        .saturating_sub(config.l2_keep_ms)
        .min(l2_dependent_watermark);
    match store.prune_rollups(Tier::L2, l2_cutoff).await {
        Ok(count) => report.pruned[1] = to_i64_count(count),
        Err(error) => record_step_error(&mut first_error, "prune L2", error),
    }

    if let Some(l3_keep_ms) = config.l3 {
        let l3_dependent_watermark = if config.l4.is_some() {
            required_watermark(initial_l4_watermark)
        } else {
            i64::MAX
        };
        let l3_cutoff = now_ms
            .saturating_sub(l3_keep_ms)
            .min(l3_dependent_watermark);
        match store.prune_rollups(Tier::L3, l3_cutoff).await {
            Ok(count) => report.pruned[2] = to_i64_count(count),
            Err(error) => record_step_error(&mut first_error, "prune L3", error),
        }
    }

    if let Some(l4_keep_ms) = config.l4.filter(|keep_ms| *keep_ms > 0) {
        let cutoff_ms = now_ms.saturating_sub(l4_keep_ms);
        if archive.queryable {
            let paths = archive_paths(store.database_path(), archive);
            for _ in 0..MAX_ARCHIVE_BATCHES_PER_TICK {
                match move_expired_l4(store, &paths, cutoff_ms, ARCHIVE_BATCH_ROWS).await {
                    Ok(0) => break,
                    Ok(count) => {
                        report.archived_l4 = report.archived_l4.saturating_add(count);
                        report.expired_l4 = report.expired_l4.saturating_add(count);
                        report.pruned[3] = report.pruned[3].saturating_add(count);
                    }
                    Err(error) => {
                        record_step_error(&mut first_error, "expire L4", error);
                        break;
                    }
                }
            }
        } else {
            match store.prune_rollups(Tier::L4, cutoff_ms).await {
                Ok(count) => {
                    report.expired_l4 = to_i64_count(count);
                    report.pruned[3] = report.expired_l4;
                }
                Err(error) => record_step_error(&mut first_error, "expire L4", error),
            }
        }
    }

    if let Some(error) = first_error {
        Err(MaintenanceError { report, error })
    } else {
        Ok(report)
    }
}

async fn promote(
    store: &SqliteHistoryStore,
    source_tier: Tier,
    target_tier: Tier,
    watermark_key: &str,
    now_ms: i64,
    grace_ms: i64,
    source_folded_until_ms: Option<i64>,
) -> Result<i64, StoreError> {
    let target_resolution_ms = target_tier.resolution_ms();
    let mut search_from_ms = store
        .history_state_get::<i64>(watermark_key)
        .await?
        .unwrap_or(i64::MIN);
    let mut promoted = 0;

    while promoted < MAX_PROMOTIONS_PER_TICK {
        let Some(source_start_ms) = store
            .oldest_tier_bucket_start_at_or_after(source_tier, search_from_ms)
            .await?
        else {
            break;
        };
        let target_start_ms = bucket_start_for(target_resolution_ms, source_start_ms);
        let target_end_ms = target_start_ms.saturating_add(target_resolution_ms);
        if !is_complete(target_start_ms, target_resolution_ms, grace_ms, now_ms) {
            break;
        }
        if source_folded_until_ms.is_some_and(|watermark| target_end_ms > watermark) {
            break;
        }

        let finer = store
            .read_tier_buckets(source_tier, target_start_ms, target_end_ms)
            .await?;
        let Some(bucket) = fold(target_start_ms, &finer) else {
            search_from_ms = target_end_ms;
            continue;
        };
        store.upsert_tier_bucket(target_tier, &bucket).await?;
        store
            .history_state_set(watermark_key, &target_end_ms, now_ms)
            .await?;
        search_from_ms = target_end_ms;
        promoted += 1;
    }

    Ok(promoted)
}

pub(crate) async fn refold_ancestors_for_late_write(
    store: &SqliteHistoryStore,
    captured_at_ms: i64,
    now_ms: i64,
    ladder: &RetentionLadder,
    new_sample: &TierBucket,
    new_raw_row: bool,
) -> Result<(), StoreError> {
    let l3_watermark = store.history_state_get::<i64>("l3FoldedUntilMs").await?;
    let l4_watermark = store.history_state_get::<i64>("l4FoldedUntilMs").await?;
    let l3_enabled = store
        .history_state_get::<bool>("l3Enabled")
        .await?
        .unwrap_or(true);
    let l4_enabled = store
        .history_state_get::<bool>("l4Enabled")
        .await?
        .unwrap_or(true);

    let l3_start_ms = bucket_start_for(Tier::L3.resolution_ms(), captured_at_ms);
    if l3_enabled && l3_watermark.is_some_and(|watermark| l3_start_ms < watermark) {
        refold_one(
            store,
            Tier::L2,
            Tier::L3,
            l3_start_ms,
            now_ms,
            ladder.l2.keep_days,
            new_sample,
            new_raw_row,
        )
        .await?;
    }

    let l4_start_ms = bucket_start_for(Tier::L4.resolution_ms(), captured_at_ms);
    if l4_enabled && l4_watermark.is_some_and(|watermark| l4_start_ms < watermark) {
        let source_tier = if l3_enabled { Tier::L3 } else { Tier::L2 };
        let source_keep_days = if source_tier == Tier::L3 {
            ladder.l3.keep_days
        } else {
            ladder.l2.keep_days
        };
        refold_one(
            store,
            source_tier,
            Tier::L4,
            l4_start_ms,
            now_ms,
            source_keep_days,
            new_sample,
            new_raw_row,
        )
        .await?;
    }
    Ok(())
}

// The struct-of-args refactor touches the T2 fold contract.
#[allow(clippy::too_many_arguments)]
async fn refold_one(
    store: &SqliteHistoryStore,
    source_tier: Tier,
    target_tier: Tier,
    target_start_ms: i64,
    now_ms: i64,
    source_keep_days: i64,
    new_sample: &TierBucket,
    new_raw_row: bool,
) -> Result<(), StoreError> {
    let target_end_ms = target_start_ms.saturating_add(target_tier.resolution_ms());
    let finer = store
        .read_tier_buckets(source_tier, target_start_ms, target_end_ms)
        .await?;
    let source_count = finer.iter().map(|bucket| bucket.sample_count).sum::<i64>();
    let source_holds_whole_range = source_count > 0
        && target_start_ms >= now_ms.saturating_sub(source_keep_days.saturating_mul(DAY_MS));
    if !source_holds_whole_range && !new_raw_row {
        return Ok(());
    }
    let bucket = if source_holds_whole_range {
        fold(target_start_ms, &finer)
    } else {
        let mut merge = store
            .read_tier_buckets(target_tier, target_start_ms, target_end_ms)
            .await?;
        merge.push(new_sample.clone());
        fold(target_start_ms, &merge)
    };
    if let Some(bucket) = bucket {
        store.upsert_tier_bucket(target_tier, &bucket).await?;
    }
    Ok(())
}

async fn read_watermark(
    store: &SqliteHistoryStore,
    key: &str,
    first_error: &mut Option<StoreError>,
) -> Option<i64> {
    match store.history_state_get(key).await {
        Ok(value) => value,
        Err(error) => {
            record_step_error(first_error, &format!("read {key}"), error);
            None
        }
    }
}

fn required_watermark(watermark: Option<i64>) -> i64 {
    watermark.unwrap_or(i64::MIN)
}

fn record_step_error(first_error: &mut Option<StoreError>, step: &str, error: StoreError) {
    eprintln!("history maintenance step {step} failed: {error}");
    if first_error.is_none() {
        *first_error = Some(error);
    }
}

fn to_i64_count(count: u64) -> i64 {
    count.min(i64::MAX as u64) as i64
}
