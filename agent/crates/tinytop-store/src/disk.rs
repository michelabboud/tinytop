use std::{
    io,
    path::{Path, PathBuf},
};

use sysinfo::Disks;

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
