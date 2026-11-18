//! Base-62 fractional indexing compatible with PostgreSQL `COLLATE "C"` ordering.
//!
//! Ranks are non-empty ASCII strings over `0-9A-Za-z` (byte order: `0 < 9 < A < Z < a < z`).
//! Inserting or moving a block only needs the rank of its immediate neighbours, so exactly one
//! row is rewritten.

use thiserror::Error;

/// Digits in ASCII / PostgreSQL `COLLATE "C"` order.
pub const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: i32 = 62;
const MID: usize = 31;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RankError {
    #[error("rank `{rank}` contains characters outside the base-62 alphabet")]
    InvalidCharacter { rank: String },
    #[error("prev rank `{prev}` is not lexicographically smaller than next rank `{next}`")]
    InvalidOrder { prev: String, next: String },
    #[error(
        "no base-62 rank exists strictly between `{prev}` and `{next}`; this adjacent pair is exhausted"
    )]
    Exhausted { prev: String, next: String },
}

/// First rank for an empty document.
pub fn rank_first() -> String {
    BASE62[MID].to_string()
}

fn digit_index(byte: u8) -> Option<i32> {
    BASE62
        .iter()
        .position(|candidate| *candidate == byte)
        .map(|index| index as i32)
}

fn digit_char(value: i32) -> char {
    let index = value.clamp(0, BASE - 1) as usize;
    BASE62.get(index).copied().unwrap_or(b'0') as char
}

fn validate(rank: &str) -> Result<(), RankError> {
    if rank.is_empty() || rank.bytes().all(|byte| digit_index(byte).is_some()) {
        Ok(())
    } else {
        Err(RankError::InvalidCharacter {
            rank: rank.to_string(),
        })
    }
}

/// Generate a rank between two optional neighbours.
///
/// This infallible entry point is convenient for callers that own valid ranks. Invalid or
/// exhausted inputs fall back to a deterministic append, while [`try_rank_between`] exposes the
/// precise error for self-healing callers.
pub fn rank_between(prev: Option<&str>, next: Option<&str>) -> String {
    match try_rank_between(prev, next) {
        Ok(rank) => rank,
        Err(_) => match (prev, next) {
            (Some(prev), _) => rank_after(Some(prev)),
            (None, Some(next)) => rank_before(Some(next)),
            (None, None) => rank_first(),
        },
    }
}

/// Fallible fractional rank generation.
pub fn try_rank_between(prev: Option<&str>, next: Option<&str>) -> Result<String, RankError> {
    let prev = prev.filter(|rank| !rank.is_empty());
    let next = next.filter(|rank| !rank.is_empty());
    if let Some(prev) = prev {
        validate(prev)?;
    }
    if let Some(next) = next {
        validate(next)?;
    }
    if let (Some(prev), Some(next)) = (prev, next)
        && prev.as_bytes() >= next.as_bytes()
    {
        return Err(RankError::InvalidOrder {
            prev: prev.to_string(),
            next: next.to_string(),
        });
    }

    match (prev, next) {
        (None, None) => Ok(rank_first()),
        (Some(prev), None) => Ok(rank_after(Some(prev))),
        (None, Some(next)) => before_without_prev(next),
        (Some(prev), Some(next)) => midpoint(prev, next),
    }
}

fn midpoint(prev: &str, next: &str) -> Result<String, RankError> {
    let prev_bytes = prev.as_bytes();
    let next_bytes = next.as_bytes();
    let mut common = 0usize;
    while prev_bytes.get(common).is_some() && prev_bytes.get(common) == next_bytes.get(common) {
        common += 1;
    }
    let mut output = prev.get(..common).unwrap_or(prev).to_string();
    let a = prev_bytes
        .get(common)
        .and_then(|byte| digit_index(*byte))
        .unwrap_or(-1);
    let b = next_bytes
        .get(common)
        .and_then(|byte| digit_index(*byte))
        .unwrap_or(BASE);

    if a < 0 {
        // `prev` is a strict prefix of `next`. Copy any leading zero digits of `next`, then step
        // below the first non-zero digit. This preserves room for future insertions above `prev`.
        let mut index = common;
        while next_bytes.get(index) == Some(&b'0') {
            output.push('0');
            index += 1;
        }
        let Some(digit) = next_bytes.get(index).and_then(|byte| digit_index(*byte)) else {
            return Err(RankError::Exhausted {
                prev: prev.to_string(),
                next: next.to_string(),
            });
        };
        let chosen = if digit > 1 { (digit - 1) / 2 } else { 0 };
        output.push(digit_char(chosen));
        if chosen == 0 {
            output.push(digit_char(MID as i32));
        }
        return finish_midpoint(prev, next, output);
    }

    if b >= BASE {
        return Err(RankError::Exhausted {
            prev: prev.to_string(),
            next: next.to_string(),
        });
    }
    debug_assert!(b > a);
    let low = a + 1;
    let high = b - 1;
    if low <= high {
        let digit = (low + high) / 2;
        output.push(digit_char(digit));
        return finish_midpoint(prev, next, output);
    }
    // `next` immediately follows `prev` at this digit. First try a short successor of `prev`
    // (incrementing its last digit); if that would reach or pass `next`, extend `prev`. Both
    // forms stay below `next` because this digit is already below `next`'s.
    let candidate = rank_after(Some(prev));
    if candidate.as_bytes() > prev.as_bytes() && candidate.as_bytes() < next.as_bytes() {
        return finish_midpoint(prev, next, candidate);
    }
    let mut extended = prev.to_string();
    extended.push(digit_char(MID as i32));
    finish_midpoint(prev, next, extended)
}

fn before_without_prev(next: &str) -> Result<String, RankError> {
    let bytes = next.as_bytes();
    let mut index = 0;
    while bytes.get(index) == Some(&b'0') {
        index += 1;
    }
    if index >= bytes.len() {
        // `next` is all zeros. A strict prefix is smaller and is the only available space,
        // but the empty rank is invalid.
        if bytes.len() > 1
            && let Some(prefix) = next.get(..bytes.len() - 1)
        {
            return Ok(prefix.to_string());
        }
        return Err(RankError::Exhausted {
            prev: String::new(),
            next: next.to_string(),
        });
    }
    let Some(digit) = bytes.get(index).and_then(|byte| digit_index(*byte)) else {
        return Err(RankError::Exhausted {
            prev: String::new(),
            next: next.to_string(),
        });
    };
    let chosen = if digit > 1 { (digit - 1) / 2 } else { 0 };
    let mut output = next[..index].to_string();
    output.push(digit_char(chosen));
    if chosen == 0 {
        output.push(digit_char(MID as i32));
    }
    if output.as_bytes() < next.as_bytes() {
        Ok(output)
    } else {
        Err(RankError::Exhausted {
            prev: String::new(),
            next: next.to_string(),
        })
    }
}

fn finish_midpoint(prev: &str, next: &str, output: String) -> Result<String, RankError> {
    if output.as_bytes() > prev.as_bytes()
        && output.as_bytes() < next.as_bytes()
        && !output.ends_with('0')
    {
        Ok(output)
    } else {
        Err(RankError::Exhausted {
            prev: prev.to_string(),
            next: next.to_string(),
        })
    }
}

/// Generate a rank greater than the last neighbour.
pub fn rank_after(prev: Option<&str>) -> String {
    let Some(prev) = prev.filter(|rank| !rank.is_empty()) else {
        return rank_first();
    };
    let bytes = prev.as_bytes();
    // Increment the rightmost increasable digit and drop everything after it. This keeps keys
    // short during long append-only agent runs while preserving strict ordering.
    for index in (0..bytes.len()).rev() {
        if let Some(digit) = bytes.get(index).and_then(|byte| digit_index(*byte))
            && digit < BASE - 1
        {
            let mut output = prev.get(..index).unwrap_or_default().to_string();
            output.push(digit_char(digit + 1));
            return output;
        }
    }
    let mut output = prev.to_string();
    output.push(digit_char(MID as i32));
    output
}

/// Generate a rank smaller than the first neighbour.
pub fn rank_before(next: Option<&str>) -> String {
    let Some(next) = next.filter(|rank| !rank.is_empty()) else {
        return rank_first();
    };
    if let Ok(rank) = try_rank_between(None, Some(next)) {
        return rank;
    }
    // `"0"` is the absolute minimum non-empty base-62 rank; no smaller rank exists.
    "0".to_string()
}

/// Assign evenly spaced, lexicographically ordered ranks to a list in memory.
pub fn ranks_for_count(count: usize) -> Vec<String> {
    if count == 0 {
        return Vec::new();
    }
    let mut ranks = Vec::with_capacity(count);
    let mut previous: Option<String> = None;
    for _ in 0..count {
        let next = rank_after(previous.as_deref());
        ranks.push(next.clone());
        previous = Some(next);
    }
    ranks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_ordered(ranks: &[String]) {
        for pair in ranks.windows(2) {
            assert!(
                pair[0].as_bytes() < pair[1].as_bytes(),
                "unordered ranks: {:?} >= {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn first_and_end_ranks_are_ordered() {
        let first = rank_first();
        let second = rank_after(Some(&first));
        let third = rank_after(Some(&second));
        assert!(first.as_bytes() < second.as_bytes());
        assert!(second.as_bytes() < third.as_bytes());
        assert!(rank_before(Some(&first)).as_bytes() < first.as_bytes());
    }

    #[test]
    fn repeated_midpoints_converge_without_collisions() {
        let mut prev = "0".to_string();
        let next = "1".to_string();
        for _ in 0..100 {
            let mid = try_rank_between(Some(&prev), Some(&next)).unwrap();
            assert!(prev.as_bytes() < mid.as_bytes());
            assert!(mid.as_bytes() < next.as_bytes());
            prev = mid;
        }
    }

    #[test]
    fn handles_prefix_neighbours() {
        let exhausted = try_rank_between(Some("A"), Some("A0"));
        assert!(matches!(exhausted, Err(RankError::Exhausted { .. })));
        let mid = try_rank_between(Some("A"), Some("A1")).unwrap();
        assert!(mid.as_bytes() > b"A" && mid.as_bytes() < b"A1");
        let mid = try_rank_between(Some("A"), Some("A0U")).unwrap();
        assert!(mid.as_bytes() > b"A" && mid.as_bytes() < b"A0U");
    }

    #[test]
    fn invalid_ranks_are_rejected() {
        assert!(matches!(
            try_rank_between(Some("a!"), None),
            Err(RankError::InvalidCharacter { .. })
        ));
        assert!(matches!(
            try_rank_between(Some("B"), Some("A")),
            Err(RankError::InvalidOrder { .. })
        ));
    }

    #[test]
    fn five_hundred_random_insertions_stay_sorted() {
        let mut ranks: Vec<String> = vec![];
        let mut seed: u64 = 0x5EED_1234_ABCD_9876;
        for iteration in 0..500 {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let position = if ranks.is_empty() {
                0
            } else {
                (seed as usize) % (ranks.len() + 1)
            };
            let prev = position.checked_sub(1).and_then(|index| ranks.get(index));
            let next = ranks.get(position);
            let rank = try_rank_between(prev.map(String::as_str), next.map(String::as_str))
                .unwrap_or_else(|error| panic!("iteration {iteration}: {error}"));
            assert!(rank.len() <= 64, "rank too long: {rank}");
            assert!(!ranks.contains(&rank), "duplicate rank: {rank}");
            ranks.insert(position, rank);
            assert_ordered(&ranks);
        }
    }

    #[test]
    fn zero_only_rank_has_no_valid_predecessor() {
        let error = try_rank_between(None, Some("0")).unwrap_err();
        assert!(matches!(error, RankError::Exhausted { .. }));
        assert_eq!(rank_before(Some("0")), "0");
    }

    #[test]
    fn end_appends_remain_short() {
        let mut last: Option<String> = None;
        for _ in 0..500 {
            let next = rank_after(last.as_deref());
            if let Some(previous) = &last {
                assert!(previous.as_bytes() < next.as_bytes());
            }
            assert!(next.len() <= 64, "rank grew too long: {next}");
            last = Some(next);
        }
    }
}
