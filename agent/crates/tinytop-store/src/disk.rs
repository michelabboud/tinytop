use std::{
    io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::json;
use sysinfo::Disks;

use crate::{
    HistoryMarkerType, SqliteHistoryStore, StoreError, history_state_set_on, record_event_on,
    retention_ladder::{DiskPressureState, RetentionLadder},
};

/// Supplies free-byte measurements for the filesystem containing a path.
pub trait FreeBytesProvider: Send + Sync {
    fn free_bytes(&self, path: &Path) -> io::Result<u64>;
}

/// Production free-byte provider backed by [`free_bytes_at`].
pub struct SysinfoFreeBytes;

impl FreeBytesProvider for SysinfoFreeBytes {
    fn free_bytes(&self, path: &Path) -> io::Result<u64> {
        free_bytes_at(path)
    }
}

/// State transition produced by a successful disk check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskTransition {
    Unchanged,
    Breached,
    Recovered,
}

/// Successful disk-check measurement and the transition it applied.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskCheckReport {
    pub path: PathBuf,
    pub free_bytes: i64,
    pub min_free_bytes: i64,
    pub database_bytes: i64,
    pub pressure: bool,
    pub transition: DiskTransition,
}

/// Return the available bytes on the filesystem that contains `path`.
///
/// `path` must exist; the migration passes the database's parent directory.
/// `sysinfo` exposes mounted disks rather than a direct path lookup, so both
/// `path` and each mount point are canonicalized before selecting the longest
/// prefix. Canonicalizing both sides matters on Windows, where `path` may use a
/// verbatim `\\?\` prefix while `sysinfo` reports the same mount without it.
/// Mounts that cannot be canonicalized are skipped. An indeterminate result is
/// an error because schema migration must fail closed before creating its
/// pre-image.
pub fn free_bytes_at(path: &Path) -> io::Result<u64> {
    let canonical_path = path.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot canonicalize {}: {error}", path.display()),
        )
    })?;
    let disks = Disks::new_with_refreshed_list();
    let mounts = disks
        .list()
        .iter()
        .filter_map(|disk| {
            disk.mount_point()
                .canonicalize()
                .ok()
                .map(|mount_point| (mount_point, disk.available_space()))
        })
        .collect::<Vec<_>>();
    free_bytes_from_mounts(&canonical_path, &mounts)
}

/// Measure the database directory and atomically apply its pressure state.
pub async fn check_disk(
    store: &SqliteHistoryStore,
    provider: &dyn FreeBytesProvider,
    ladder: &RetentionLadder,
    now_ms: i64,
) -> Result<DiskCheckReport, StoreError> {
    let path = match store.database_path().parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    apply_disk_measurement(store, path, provider.free_bytes(path), ladder, now_ms).await
}

/// Atomically apply an already-taken free-byte measurement.
///
/// An indeterminate measurement writes nothing and returns [`StoreError::DiskCheck`].
pub async fn apply_disk_measurement(
    store: &SqliteHistoryStore,
    path: &Path,
    measurement: io::Result<u64>,
    ladder: &RetentionLadder,
    now_ms: i64,
) -> Result<DiskCheckReport, StoreError> {
    let free_bytes = measurement
        .map(|bytes| bytes.min(i64::MAX as u64) as i64)
        .map_err(|source| StoreError::DiskCheck {
            path: path.to_path_buf(),
            source,
        })?;
    let database_bytes = store.database_bytes().await?;
    let min_free_bytes = ladder.disk_check.min_free_bytes;
    let pressure = free_bytes < min_free_bytes;
    let previous = store
        .history_state_get::<DiskPressureState>("diskPressure")
        .await?
        .unwrap_or_default();
    let transition = match (previous.active, pressure) {
        (false, true) => DiskTransition::Breached,
        (true, false) => DiskTransition::Recovered,
        _ => DiskTransition::Unchanged,
    };
    let state = DiskPressureState {
        active: pressure,
        since_ms: if pressure {
            if previous.active {
                previous.since_ms
            } else {
                Some(now_ms)
            }
        } else {
            None
        },
        free_bytes,
        min_free_bytes,
    };
    let details = json!({
        "freeBytes": free_bytes,
        "minFreeBytes": min_free_bytes,
        "databaseBytes": database_bytes,
        "path": path.display().to_string(),
    });

    let mut transaction = store.pool.begin().await?;
    history_state_set_on(&mut *transaction, "diskPressure", &state, now_ms).await?;
    history_state_set_on(&mut *transaction, "lastDiskCheckMs", &now_ms, now_ms).await?;
    match transition {
        DiskTransition::Breached => {
            record_event_on(
                &mut *transaction,
                now_ms,
                HistoryMarkerType::DiskPressure,
                &format!("Disk pressure: free {free_bytes} < minFreeBytes {min_free_bytes}"),
                details,
            )
            .await?;
        }
        DiskTransition::Recovered => {
            record_event_on(
                &mut *transaction,
                now_ms,
                HistoryMarkerType::DiskRecovered,
                &format!(
                    "Disk pressure cleared: free {free_bytes} ≥ minFreeBytes {min_free_bytes}"
                ),
                details,
            )
            .await?;
        }
        DiskTransition::Unchanged => {}
    }
    transaction.commit().await?;

    Ok(DiskCheckReport {
        path: path.to_path_buf(),
        free_bytes,
        min_free_bytes,
        database_bytes,
        pressure,
        transition,
    })
}

pub(crate) fn free_bytes_from_mounts(path: &Path, mounts: &[(PathBuf, u64)]) -> io::Result<u64> {
    mounts
        .iter()
        .filter(|(mount_point, _)| path.starts_with(mount_point))
        .max_by_key(|(mount_point, _)| mount_point.components().count())
        .map(|(_, available_space)| *available_space)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no mounted filesystem contains {}", path.display()),
            )
        })
}

pub(crate) fn required_pre_image_bytes(database_bytes: u64) -> u64 {
    database_bytes.saturating_add(database_bytes / 5)
}

pub(crate) fn has_pre_image_headroom(database_bytes: u64, free_bytes: u64) -> bool {
    free_bytes >= required_pre_image_bytes(database_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_matching_mount_prefix_wins() {
        let mounts = vec![
            (PathBuf::from("root"), 100),
            (PathBuf::from("root/data"), 200),
            (PathBuf::from("root/data/archive"), 300),
        ];

        assert_eq!(
            free_bytes_from_mounts(Path::new("root/data/history/db.sqlite"), &mounts)
                .expect("matching mount"),
            200
        );
    }

    #[test]
    fn mount_prefix_matching_respects_component_boundaries() {
        let mounts = vec![(PathBuf::from("/home"), 200), (PathBuf::from("/"), 100)];

        assert_eq!(
            free_bytes_from_mounts(Path::new("/homeless/x/db"), &mounts)
                .expect("root mount should contain the path"),
            100
        );
    }

    #[test]
    fn empty_mount_list_is_not_found() {
        let error = free_bytes_from_mounts(Path::new("root/data"), &[])
            .expect_err("empty mount list must fail closed");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("root/data"));
    }

    #[test]
    fn one_point_one_nine_headroom_is_refused() {
        assert!(!has_pre_image_headroom(100, 119));
    }

    #[test]
    fn one_point_two_headroom_is_accepted() {
        assert!(has_pre_image_headroom(100, 120));
    }
}
