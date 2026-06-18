//! The stake-weighted median of reporter price reveals (spec 03 §5.5):
//! the decentralised, manipulation-resistant bootstrap anchor. Weight is
//! **bonded stake**, not headcount — so Sybils gain nothing, and moving
//! the median requires **> 50 % of stake**, not a majority of reporters.
//!
//! Pure: the commit-reveal transport, bond economics, and band-based
//! reward/slash (§5.5) live in the daemon; this is just the aggregation.

/// One reporter's revealed price and the stake backing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub price_usd: u32,
    pub stake: u64,
}

/// The stake-weighted median: sort by price, walk cumulative stake, and
/// return the first price whose cumulative stake reaches half the total
/// (the lower-middle convention for even splits, matching §5.2). A
/// coalition needs **> 50 %** of total stake to determine which price
/// crosses the threshold. `None` if there are no reports or zero stake.
pub fn stake_weighted_median(reports: &[Report]) -> Option<u32> {
    let total: u128 = reports.iter().map(|r| u128::from(r.stake)).sum();
    if total == 0 {
        return None;
    }
    let mut sorted: Vec<&Report> = reports.iter().collect();
    sorted.sort_by_key(|r| r.price_usd);
    let threshold = (total + 1) / 2; // > 50 % to move it; lower-middle for even
    let mut acc: u128 = 0;
    for r in sorted {
        acc += u128::from(r.stake);
        if acc >= threshold {
            return Some(r.price_usd);
        }
    }
    unreachable!("cumulative stake reaches the total, which is >= threshold")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(price_usd: u32, stake: u64) -> Report {
        Report { price_usd, stake }
    }

    #[test]
    fn empty_or_zero_stake_is_none() {
        assert_eq!(stake_weighted_median(&[]), None);
        assert_eq!(stake_weighted_median(&[r(60_000, 0), r(61_000, 0)]), None);
    }

    #[test]
    fn equal_stakes_take_the_lower_middle() {
        // Odd count → the middle; even count → the lower-middle (§5.2).
        assert_eq!(stake_weighted_median(&[r(30, 1), r(10, 1), r(20, 1)]), Some(20));
        assert_eq!(stake_weighted_median(&[r(40, 1), r(10, 1), r(30, 1), r(20, 1)]), Some(20));
    }

    #[test]
    fn a_majority_stake_sets_the_median_alone() {
        // One reporter with > 50 % of stake fixes the median regardless of
        // how many small reporters disagree.
        let reports = [r(64_000, 100), r(1, 10), r(2, 10), r(3, 10), r(1_000_000, 10)];
        assert_eq!(stake_weighted_median(&reports), Some(64_000));
    }

    #[test]
    fn sybils_cannot_move_it_without_stake() {
        // 100 zero-ish-stake Sybils at an absurd price do not outweigh one
        // honest, heavily-bonded reporter.
        let mut reports = vec![r(64_000, 1_000_000)];
        for _ in 0..100 {
            reports.push(r(1, 1));
        }
        assert_eq!(stake_weighted_median(&reports), Some(64_000));
    }

    #[test]
    fn stake_weighting_shifts_the_crossing() {
        // 60 % of stake sits at 62k, so the median is 62k even though a
        // higher price exists with less stake behind it.
        let reports = [r(60_000, 20), r(62_000, 60), r(70_000, 20)];
        assert_eq!(stake_weighted_median(&reports), Some(62_000));
    }
}
