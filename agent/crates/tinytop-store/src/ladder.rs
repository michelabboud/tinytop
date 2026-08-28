#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    L1,
    L2,
    L3,
    L4,
}

impl Tier {
    pub fn resolution_ms(self) -> i64 {
        match self {
            Self::L1 => 0,
            Self::L2 => 60_000,
            Self::L3 => 300_000,
            Self::L4 => 3_600_000,
        }
    }

    pub fn table(self) -> &'static str {
        match self {
            Self::L1 => "metric_samples",
            Self::L2 => "metric_rollups_1m",
            Self::L3 => "metric_rollups_5m",
            Self::L4 => "metric_rollups_1h",
        }
    }

    pub fn finer(self) -> Option<Self> {
        match self {
            Self::L1 => None,
            Self::L2 => Some(Self::L1),
            Self::L3 => Some(Self::L2),
            Self::L4 => Some(Self::L3),
        }
    }

    pub fn coarser(self) -> Option<Self> {
        match self {
            Self::L1 => Some(Self::L2),
            Self::L2 => Some(Self::L3),
            Self::L3 => Some(Self::L4),
            Self::L4 => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stat {
    pub avg: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TierBucket {
    pub bucket_start_ms: i64,
    pub first_captured_at_ms: i64,
    pub newest_captured_at_ms: i64,
    pub sample_count: i64,
    pub cpu: Stat,
    pub memory: Stat,
    pub swap: Stat,
    pub load: Stat,
    pub root_used: Option<Stat>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawSampleRow {
    pub captured_at_ms: i64,
    pub cpu_usage_percent: f64,
    pub memory_used_percent: f64,
    pub swap_used_percent: f64,
    pub load_percent: f64,
    pub root_used_percent: Option<f64>,
}

pub fn fold(bucket_start_ms: i64, finer: &[TierBucket]) -> Option<TierBucket> {
    let first = finer.first()?;
    let sample_count = finer.iter().map(|bucket| bucket.sample_count).sum::<i64>();
    debug_assert!(
        sample_count > 0,
        "stored tier buckets must represent samples"
    );
    if sample_count <= 0 {
        return None;
    }

    let fold_stat = |get: fn(&TierBucket) -> Stat| Stat {
        avg: finer
            .iter()
            .map(|bucket| get(bucket).avg * bucket.sample_count as f64)
            .sum::<f64>()
            / sample_count as f64,
        min: finer
            .iter()
            .map(|bucket| get(bucket).min)
            .fold(f64::INFINITY, f64::min),
        max: finer
            .iter()
            .map(|bucket| get(bucket).max)
            .fold(f64::NEG_INFINITY, f64::max),
    };

    let root_sample_count = finer
        .iter()
        .filter(|bucket| bucket.root_used.is_some())
        .map(|bucket| bucket.sample_count)
        .sum::<i64>();
    let root_used = (root_sample_count > 0).then(|| Stat {
        avg: finer
            .iter()
            .filter_map(|bucket| {
                bucket
                    .root_used
                    .map(|stat| stat.avg * bucket.sample_count as f64)
            })
            .sum::<f64>()
            / root_sample_count as f64,
        min: finer
            .iter()
            .filter_map(|bucket| bucket.root_used.map(|stat| stat.min))
            .fold(f64::INFINITY, f64::min),
        max: finer
            .iter()
            .filter_map(|bucket| bucket.root_used.map(|stat| stat.max))
            .fold(f64::NEG_INFINITY, f64::max),
    });

    Some(TierBucket {
        bucket_start_ms,
        first_captured_at_ms: finer
            .iter()
            .map(|bucket| bucket.first_captured_at_ms)
            .min()
            .unwrap_or(first.first_captured_at_ms),
        newest_captured_at_ms: finer
            .iter()
            .map(|bucket| bucket.newest_captured_at_ms)
            .max()
            .unwrap_or(first.newest_captured_at_ms),
        sample_count,
        cpu: fold_stat(|bucket| bucket.cpu),
        memory: fold_stat(|bucket| bucket.memory),
        swap: fold_stat(|bucket| bucket.swap),
        load: fold_stat(|bucket| bucket.load),
        root_used,
    })
}

pub fn raw_to_bucket(sample: &RawSampleRow) -> TierBucket {
    let stat = |value| Stat {
        avg: value,
        min: value,
        max: value,
    };
    TierBucket {
        bucket_start_ms: sample.captured_at_ms,
        first_captured_at_ms: sample.captured_at_ms,
        newest_captured_at_ms: sample.captured_at_ms,
        sample_count: 1,
        cpu: stat(sample.cpu_usage_percent),
        memory: stat(sample.memory_used_percent),
        swap: stat(sample.swap_used_percent),
        load: stat(sample.load_percent),
        root_used: sample.root_used_percent.map(stat),
    }
}

pub fn bucket_start_for(resolution_ms: i64, timestamp_ms: i64) -> i64 {
    debug_assert!(resolution_ms > 0, "bucket resolution must be positive");
    timestamp_ms
        .div_euclid(resolution_ms)
        .saturating_mul(resolution_ms)
}

pub fn is_complete(bucket_start_ms: i64, resolution_ms: i64, grace_ms: i64, now_ms: i64) -> bool {
    bucket_start_ms
        .saturating_add(resolution_ms)
        .saturating_add(grace_ms)
        <= now_ms
}

pub fn grace_ms(poll_interval_ms: i64) -> i64 {
    3_000.max(poll_interval_ms.saturating_mul(2))
}
