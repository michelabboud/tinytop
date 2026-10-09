//! Which processes a sample keeps, and in what order.
//!
//! A sample used to be the top N processes by CPU. That list missed the
//! processes holding the memory: on 2026-10-09 only 7 of the top 12 by memory
//! were also in the top 12 by CPU, and a process that had been swapped out
//! (0.1 GB resident, 2.6 GB in swap) would not have ranked by resident memory
//! either. A sample is now the union of two lists, each `top_process_count`
//! long: the top by CPU and the top by memory, where memory is resident plus
//! swapped. A process in both lists appears once and carries both ranks.
//!
//! Every platform collector goes through [`rank_processes`], so the selection
//! and the ordering cannot differ between the Linux path and the `sysinfo`
//! path used on macOS and Windows.

use std::cmp::Ordering;

use tinytop_types::ProcessSnapshot;

/// The memory a process is charged with when ranking: resident plus swapped.
/// Unknown swap counts as zero, so on a platform that never reports swap the
/// memory list is simply the resident-memory list.
pub fn memory_footprint_bytes(process: &ProcessSnapshot) -> u64 {
    process
        .rss_bytes
        .saturating_add(process.swap_bytes.unwrap_or(0))
}

/// Reduce every process on the host to the set one sample keeps.
///
/// The result is the union of the top `top_process_count` by CPU and the top
/// `top_process_count` by memory, so it holds between N and 2N rows (fewer
/// when the host has fewer processes). `cpu_rank` and `memory_rank` are set on
/// every returned row — zero-based, `None` when the row is not in that list —
/// and any rank the candidates arrived with is discarded.
///
/// **Order of the returned rows:** the CPU list first, in CPU order, then the
/// processes that are only in the memory list, in memory order. The first
/// `min(N, process count)` rows are therefore exactly the CPU list, which is
/// what a consumer written before the memory list existed reads.
///
/// **Ties are broken deterministically**, never by the iteration order of the
/// process table:
/// - CPU list: CPU descending, then memory footprint descending, then pid
///   ascending.
/// - Memory list: memory footprint descending, then CPU descending, then pid
///   ascending.
///
/// A CPU value that is not a number sorts below every real value. A
/// `top_process_count` of zero is treated as one, the same floor the
/// collectors apply to a setting that bypassed validation.
pub fn rank_processes(
    candidates: Vec<ProcessSnapshot>,
    top_process_count: usize,
) -> Vec<ProcessSnapshot> {
    let top_process_count = top_process_count.max(1);

    let mut by_cpu = (0..candidates.len()).collect::<Vec<_>>();
    by_cpu.sort_by(|&left, &right| cpu_order(&candidates[left], &candidates[right]));
    by_cpu.truncate(top_process_count);

    let mut by_memory = (0..candidates.len()).collect::<Vec<_>>();
    by_memory.sort_by(|&left, &right| memory_order(&candidates[left], &candidates[right]));
    by_memory.truncate(top_process_count);

    let mut slots = candidates
        .into_iter()
        .map(|mut process| {
            process.cpu_rank = None;
            process.memory_rank = None;
            Some(process)
        })
        .collect::<Vec<_>>();
    for (rank, &index) in by_cpu.iter().enumerate() {
        if let Some(process) = &mut slots[index] {
            process.cpu_rank = Some(rank_value(rank));
        }
    }
    for (rank, &index) in by_memory.iter().enumerate() {
        if let Some(process) = &mut slots[index] {
            process.memory_rank = Some(rank_value(rank));
        }
    }

    // Taking a slot empties it, so a process in both lists is emitted once:
    // with the CPU list, where the second pass finds nothing left to take.
    by_cpu
        .into_iter()
        .chain(by_memory)
        .filter_map(|index| slots[index].take())
        .collect()
}

fn cpu_order(left: &ProcessSnapshot, right: &ProcessSnapshot) -> Ordering {
    cpu_key(right)
        .total_cmp(&cpu_key(left))
        .then_with(|| memory_footprint_bytes(right).cmp(&memory_footprint_bytes(left)))
        .then_with(|| left.pid.cmp(&right.pid))
}

fn memory_order(left: &ProcessSnapshot, right: &ProcessSnapshot) -> Ordering {
    memory_footprint_bytes(right)
        .cmp(&memory_footprint_bytes(left))
        .then_with(|| cpu_key(right).total_cmp(&cpu_key(left)))
        .then_with(|| left.pid.cmp(&right.pid))
}

/// `total_cmp` orders `-0.0` below `0.0` and a NaN above or below everything
/// depending on its sign bit; neither is a ranking anyone means. Adding `0.0`
/// folds the two zeros together, and a NaN is pinned to the bottom.
fn cpu_key(process: &ProcessSnapshot) -> f64 {
    if process.cpu_percent.is_nan() {
        f64::NEG_INFINITY
    } else {
        process.cpu_percent + 0.0
    }
}

fn rank_value(rank: usize) -> u32 {
    // A list is at most `top_process_count` long (1–50 once validated); the
    // saturation only keeps a direct caller with an absurd count from wrapping.
    u32::try_from(rank).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use tinytop_types::ProcessSnapshot;

    use super::{memory_footprint_bytes, rank_processes};

    const MIB: u64 = 1024 * 1024;

    fn process(pid: u32, cpu_percent: f64, rss_mib: u64, swap_mib: Option<u64>) -> ProcessSnapshot {
        ProcessSnapshot {
            pid,
            command: format!("fixture-{pid}"),
            cpu_percent,
            memory_percent: 0.0,
            rss_bytes: rss_mib * MIB,
            parent_pid: None,
            started_at: None,
            gpu_percent: None,
            swap_bytes: swap_mib.map(|mib| mib * MIB),
            cpu_rank: None,
            memory_rank: None,
        }
    }

    fn pids(processes: &[ProcessSnapshot]) -> Vec<u32> {
        processes.iter().map(|process| process.pid).collect()
    }

    fn ranks(processes: &[ProcessSnapshot]) -> Vec<(u32, Option<u32>, Option<u32>)> {
        processes
            .iter()
            .map(|process| (process.pid, process.cpu_rank, process.memory_rank))
            .collect()
    }

    /// The list a sample held before the memory list existed: CPU descending,
    /// first N. Fixtures that use it have no CPU ties, so the old sort (which
    /// left ties in process-table order) has exactly one answer.
    fn old_cpu_list(mut candidates: Vec<ProcessSnapshot>, count: usize) -> Vec<u32> {
        candidates.sort_by(|left, right| {
            right
                .cpu_percent
                .partial_cmp(&left.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(count);
        pids(&candidates)
    }

    /// Six processes: two busy and small, two idle and large, one both, one
    /// neither. No ties on either measure.
    fn mixed_host() -> Vec<ProcessSnapshot> {
        vec![
            process(10, 90.0, 10, Some(0)),   // CPU only
            process(20, 80.0, 900, Some(0)),  // both
            process(30, 70.0, 20, Some(0)),   // CPU only
            process(40, 1.0, 800, Some(0)),   // memory only
            process(50, 0.5, 700, Some(150)), // memory only
            process(60, 0.1, 5, Some(0)),     // neither
        ]
    }

    #[test]
    fn a_sample_is_the_union_of_the_cpu_list_and_the_memory_list() {
        let ranked = rank_processes(mixed_host(), 3);

        assert_eq!(
            ranks(&ranked),
            vec![
                (10, Some(0), None),    // CPU only
                (20, Some(1), Some(0)), // in both lists, emitted once
                (30, Some(2), None),    // CPU only
                (50, None, Some(1)),    // memory only: 700 + 150 beats 800
                (40, None, Some(2)),    // memory only
            ]
        );
        // The process in neither list is not in the sample.
        assert!(!pids(&ranked).contains(&60));
    }

    #[test]
    fn a_process_in_both_lists_appears_once() {
        let ranked = rank_processes(mixed_host(), 3);
        let mut seen = pids(&ranked);
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), ranked.len());
        assert_eq!(ranked.iter().filter(|process| process.pid == 20).count(), 1);
    }

    #[test]
    fn the_first_rows_are_exactly_the_old_cpu_list() {
        // Break caught: emitting in memory order, or interleaving the lists,
        // hands a consumer that reads the first N rows something other than
        // the CPU ranking it has always read.
        for count in [1, 2, 3, 4, 6] {
            let ranked = rank_processes(mixed_host(), count);
            let expected = old_cpu_list(mixed_host(), count);
            assert_eq!(
                pids(&ranked[..expected.len()]),
                expected,
                "top_process_count={count}"
            );
            for (index, process) in ranked[..expected.len()].iter().enumerate() {
                assert_eq!(process.cpu_rank, Some(index as u32), "count={count}");
            }
            // Everything after the CPU list is memory-only, in memory order.
            let tail = &ranked[expected.len()..];
            assert!(tail.iter().all(|process| process.cpu_rank.is_none()));
            let tail_ranks = tail
                .iter()
                .map(|process| process.memory_rank.expect("tail rows are memory-ranked"))
                .collect::<Vec<_>>();
            assert!(tail_ranks.is_sorted(), "count={count}: {tail_ranks:?}");
        }
    }

    #[test]
    fn a_swapped_out_process_ranks_by_memory_and_is_absent_from_the_cpu_list() {
        // The 2026-10-09 shape: 0.1 GB resident, 2.6 GB in swap, idle.
        let mut host = mixed_host();
        host.push(process(70, 0.0, 100, Some(2_600)));

        let ranked = rank_processes(host, 3);
        let swapped = ranked
            .iter()
            .find(|process| process.pid == 70)
            .expect("the swapped-out process is in the sample");
        assert_eq!(swapped.memory_rank, Some(0));
        assert_eq!(swapped.cpu_rank, None);
        assert_eq!(swapped.swap_bytes, Some(2_600 * MIB));
        assert_eq!(memory_footprint_bytes(swapped), 2_700 * MIB);
        // It is emitted after the CPU list, first among the memory-only rows.
        assert_eq!(pids(&ranked), vec![10, 20, 30, 70, 50]);
        assert_eq!(ranked[1].memory_rank, Some(1));
    }

    #[test]
    fn without_its_swap_the_same_process_would_not_be_in_the_sample() {
        // The counterpart of the test above: it is the swap, not the 100 MiB
        // resident, that puts pid 70 in the memory list.
        let mut host = mixed_host();
        host.push(process(70, 0.0, 100, None));
        assert!(!pids(&rank_processes(host, 3)).contains(&70));
    }

    #[test]
    fn unknown_swap_everywhere_ranks_memory_by_resident_bytes() {
        let host = mixed_host()
            .into_iter()
            .map(|mut process| {
                process.swap_bytes = None;
                process
            })
            .collect::<Vec<_>>();

        let ranked = rank_processes(host, 3);
        // By resident memory alone: 20 (900), 40 (800), 50 (700). With swap
        // known, 50 outranked 40.
        assert_eq!(
            ranks(&ranked),
            vec![
                (10, Some(0), None),
                (20, Some(1), Some(0)),
                (30, Some(2), None),
                (40, None, Some(1)),
                (50, None, Some(2)),
            ]
        );
        // Unknown stays unknown; ranking never turns it into a zero.
        assert!(ranked.iter().all(|process| process.swap_bytes.is_none()));
    }

    #[test]
    fn a_count_of_one_keeps_one_row_per_list() {
        let ranked = rank_processes(mixed_host(), 1);
        assert_eq!(
            ranks(&ranked),
            vec![(10, Some(0), None), (20, None, Some(0))]
        );

        // When the busiest process is also the largest, one row carries both.
        let mut host = mixed_host();
        host[0].rss_bytes = 5_000 * MIB;
        let ranked = rank_processes(host, 1);
        assert_eq!(ranks(&ranked), vec![(10, Some(0), Some(0))]);
    }

    #[test]
    fn a_count_above_the_process_count_keeps_every_process_once() {
        let ranked = rank_processes(mixed_host(), 50);
        assert_eq!(ranked.len(), 6);
        assert_eq!(pids(&ranked), old_cpu_list(mixed_host(), 50));
        assert!(ranked.iter().all(|process| process.cpu_rank.is_some()));
        let mut memory_ranks = ranked
            .iter()
            .map(|process| process.memory_rank.expect("every process is memory-ranked"))
            .collect::<Vec<_>>();
        memory_ranks.sort_unstable();
        assert_eq!(memory_ranks, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_sample_holds_between_n_and_two_n_rows() {
        for count in 1..=6 {
            let ranked = rank_processes(mixed_host(), count);
            let floor = count.min(6);
            let ceiling = (2 * count).min(6);
            assert!(
                (floor..=ceiling).contains(&ranked.len()),
                "count={count} kept {} rows",
                ranked.len()
            );
            let cpu_ranked = ranked.iter().filter(|p| p.cpu_rank.is_some()).count();
            let memory_ranked = ranked.iter().filter(|p| p.memory_rank.is_some()).count();
            assert_eq!(cpu_ranked, floor, "count={count}");
            assert_eq!(memory_ranked, floor, "count={count}");
        }
    }

    #[test]
    fn no_processes_and_a_zero_count_are_handled() {
        assert!(rank_processes(Vec::new(), 12).is_empty());
        // Zero is floored to one, as the collectors do for an unvalidated count.
        let ranked = rank_processes(mixed_host(), 0);
        assert_eq!(ranked, rank_processes(mixed_host(), 1));
    }

    #[test]
    fn ties_are_broken_the_same_way_whatever_order_the_processes_arrive_in() {
        // Break caught: a tie left to the process table's iteration order (a
        // hash map) reshuffles idle processes between two identical ticks.
        let host = vec![
            process(5, 0.0, 10, None),
            process(3, 0.0, 10, None),
            process(9, 0.0, 30, None),
            process(1, 0.0, 10, None),
            process(7, 2.0, 30, None),
        ];
        let mut reversed = host.clone();
        reversed.reverse();
        let mut rotated = host.clone();
        rotated.rotate_left(2);

        let ranked = rank_processes(host, 4);
        assert_eq!(ranked, rank_processes(reversed, 4));
        assert_eq!(ranked, rank_processes(rotated, 4));
        assert_eq!(
            ranks(&ranked),
            vec![
                // CPU: 7 leads; the idle four tie on CPU, so the larger (9)
                // comes first and the equal-sized rest go by pid.
                (7, Some(0), Some(0)),
                (9, Some(1), Some(1)),
                (1, Some(2), Some(2)),
                (3, Some(3), Some(3)),
            ]
        );
        // Memory: 7 and 9 tie on footprint, so the busier (7) comes first.
    }

    #[test]
    fn a_cpu_value_that_is_not_a_number_ranks_last_and_negative_zero_is_zero() {
        let host = vec![
            process(1, f64::NAN, 1, None),
            process(2, -0.0, 1, None),
            process(3, 0.0, 1, None),
            process(4, 0.1, 1, None),
        ];
        let ranked = rank_processes(host, 4);
        // 2 and 3 are the same CPU value and the same size: pid decides.
        assert_eq!(pids(&ranked), vec![4, 2, 3, 1]);
    }

    #[test]
    fn ranks_the_candidates_arrived_with_are_replaced() {
        let mut host = mixed_host();
        host[5].cpu_rank = Some(0);
        host[5].memory_rank = Some(0);
        host[0].memory_rank = Some(3);

        let ranked = rank_processes(host, 3);
        assert!(!pids(&ranked).contains(&60));
        assert_eq!(ranked[0].pid, 10);
        assert_eq!(ranked[0].memory_rank, None);
    }

    #[test]
    fn a_footprint_that_would_overflow_saturates() {
        let mut huge = process(1, 0.0, 0, None);
        huge.rss_bytes = u64::MAX;
        huge.swap_bytes = Some(u64::MAX);
        assert_eq!(memory_footprint_bytes(&huge), u64::MAX);
    }
}
