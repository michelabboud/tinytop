use tinytop_store::ladder::{
    Stat, Tier, TierBucket, bucket_start_for, fold, grace_ms, is_complete,
};

fn bucket(start: i64, count: i64, avg: f64, min: f64, max: f64) -> TierBucket {
    let stat = Stat { avg, min, max };
    TierBucket {
        bucket_start_ms: start,
        first_captured_at_ms: start,
        newest_captured_at_ms: start + 59_000,
        sample_count: count,
        cpu: stat,
        memory: stat,
        swap: stat,
        load: stat,
        root_used: None,
    }
}

#[test]
fn fold_weights_by_sample_count_not_average_of_averages() {
    // Break caught: coarse averages are computed as an average of bucket
    // averages instead of preserving the number of raw samples represented.
    let output = fold(
        0,
        &[
            bucket(0, 40, 10.0, 5.0, 20.0),
            bucket(60_000, 3, 100.0, 90.0, 100.0),
        ],
    )
    .expect("non-empty input should fold");

    assert_eq!(output.sample_count, 43);
    assert!((output.cpu.avg - (10.0 * 40.0 + 100.0 * 3.0) / 43.0).abs() < 1e-9);
    assert_eq!(output.cpu.min, 5.0);
    assert_eq!(output.cpu.max, 100.0);
    assert_eq!(output.first_captured_at_ms, 0);
    assert_eq!(output.newest_captured_at_ms, 119_000);
}

#[test]
fn fold_of_empty_is_none() {
    // Break caught: an empty source range fabricates a zero-valued bucket.
    assert!(fold(0, &[]).is_none());
}

#[test]
fn fold_root_used_ignores_buckets_without_a_value() {
    // Break caught: filesystems that did not report a root mount dilute the
    // average or turn a real root utilization aggregate into NULL.
    let mut with_root = bucket(0, 10, 10.0, 10.0, 10.0);
    with_root.root_used = Some(Stat {
        avg: 50.0,
        min: 50.0,
        max: 50.0,
    });
    let without_root = bucket(60_000, 30, 20.0, 20.0, 20.0);

    let output = fold(0, &[with_root, without_root]).expect("non-empty input should fold");

    assert_eq!(
        output.root_used,
        Some(Stat {
            avg: 50.0,
            min: 50.0,
            max: 50.0,
        })
    );
}

#[test]
fn tier_navigation_and_bucket_completion_follow_fixed_resolutions() {
    // Break caught: tier traversal, negative timestamp bucketing, or the grace
    // boundary changes and maintenance freezes a bucket too early.
    assert_eq!(Tier::L2.resolution_ms(), 60_000);
    assert_eq!(Tier::L3.resolution_ms(), 300_000);
    assert_eq!(Tier::L4.resolution_ms(), 3_600_000);
    assert_eq!(Tier::L2.finer(), Some(Tier::L1));
    assert_eq!(Tier::L3.coarser(), Some(Tier::L4));
    assert_eq!(bucket_start_for(60_000, -1), -60_000);
    assert_eq!(grace_ms(1_500), 3_000);
    assert_eq!(grace_ms(2_000), 4_000);
    assert!(!is_complete(0, 60_000, 3_000, 62_999));
    assert!(is_complete(0, 60_000, 3_000, 63_000));
}
