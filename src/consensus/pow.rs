use crate::types::block::BlockHeader;

// ======================================================================
// Difficulty Target System
// ======================================================================
// We store difficulty as a compact "target" in the block header.
// The miner must find a hash <= target.
//
// Compact format (same concept as Bitcoin's nBits):
//   Byte 0 (MSB): exponent E
//   Bytes 1-3: mantissa M (3 bytes, big-endian)
//   Target = M * 256^(E-3)
//
// In a 32-byte big-endian array:
//   The mantissa is placed starting at byte (32 - E).

/// Expand compact target to 32-byte big-endian target.
pub fn compact_to_target(compact: u32) -> [u8; 32] {
    let exp = (compact >> 24) as usize;
    let mantissa = compact & 0x00FF_FFFF;

    let mut target = [0u8; 32];
    if exp == 0 || mantissa == 0 {
        return target;
    }

    let m = [
        ((mantissa >> 16) & 0xFF) as u8,
        ((mantissa >> 8) & 0xFF) as u8,
        (mantissa & 0xFF) as u8,
    ];

    // Place mantissa so that it starts at byte index (32 - exp)
    let start = 32usize.saturating_sub(exp);
    for (i, &b) in m.iter().enumerate() {
        let pos = start + i;
        if pos < 32 {
            target[pos] = b;
        }
    }
    target
}

/// Compress a 32-byte target back to compact form.
pub fn target_to_compact(target: &[u8; 32]) -> u32 {
    // Find first non-zero byte
    let mut first = 32usize;
    for (i, &b) in target.iter().enumerate() {
        if b != 0 {
            first = i;
            break;
        }
    }
    if first == 32 {
        return 0;
    }

    let exp = (32 - first) as u32;
    let mut mantissa: u32 = 0;
    for i in 0..3 {
        let pos = first + i;
        let b = if pos < 32 { target[pos] as u32 } else { 0 };
        mantissa = (mantissa << 8) | b;
    }
    (exp << 24) | (mantissa & 0x00FF_FFFF)
}

/// Check if a block header hash meets the difficulty target.
/// Hash (big-endian) must be <= target.
pub fn check_pow(header: &BlockHeader) -> bool {
    let hash = header.hash();
    let target = compact_to_target(header.difficulty);
    hash_le_target(&hash, &target)
}

fn hash_le_target(hash: &[u8; 32], target: &[u8; 32]) -> bool {
    for i in 0..32 {
        if hash[i] < target[i] { return true; }
        if hash[i] > target[i] { return false; }
    }
    true
}

/// Mine a block header by iterating nonces.
pub fn mine(header: &mut BlockHeader) -> Option<u32> {
    for nonce in 0..=u32::MAX {
        header.nonce = nonce;
        if check_pow(header) {
            return Some(nonce);
        }
    }
    None
}

/// Block reward with halving every 210,000 blocks.
pub fn block_reward(height: u32) -> u64 {
    let halvings = height / 210_000;
    if halvings >= 64 { return 0; }
    5000u64 >> halvings
}

// ======================================================================
// LWMA (Linear Weighted Moving Average) Difficulty Adjustment
// ======================================================================
// Based on Zawy's LWMA used by Grin, Masari, Haven, etc.
//
// Each block i in the window gets weight i (most recent = highest weight).
// This responds quickly to hashrate changes while staying smooth.
//
// We work in "difficulty units" where difficulty = number of hashes
// expected to find a valid block. For a target T in 256-bit space:
//   difficulty ≈ 2^256 / T
// But since we can't do 2^256 math easily, we use a scaled approach:
//   difficulty_score = (MAX_COMPACT_MANTISSA + 1) / target_mantissa
//   adjusted for exponent differences.
//
// Simpler approach: we track difficulty as a u64 score derived from
// the compact target. Lower target = higher difficulty score.

/// LWMA window size.
pub const LWMA_WINDOW: usize = 60;

/// Target block time in seconds.
pub const TARGET_BLOCK_TIME: u64 = 60;

/// Initial difficulty (compact target).
/// 0x2000FFFF means exp=32, mantissa=0x00FFFF.
/// Target = 0x00FFFF * 256^29 — very easy, first byte of hash just needs to be 0x00.
/// Actually let's use something that requires ~first byte = 0x00:
/// exp=0x1F (31), mantissa=0x00FFFF → target starts at byte 1: 00 FF FF 00...
/// This means hash[0] must be 0 → ~1/256 chance → finds in ~256 nonces.
pub const INITIAL_DIFFICULTY: u32 = 0x1F00_FFFF;

/// Easiest possible difficulty (max target).
pub const MIN_DIFFICULTY: u32 = 0x2000_FFFF;

/// Convert compact target to a difficulty score (u64).
/// Higher score = harder. Score ≈ 2^(24) / mantissa * 256^(32-exp).
/// Simplified: we use the number of leading zero bytes * 256 + inverse mantissa ratio.
/// Actually, the simplest working approach:
///   score = (32 - first_nonzero_byte_position) * 0x1000000 + (0xFFFFFF - mantissa)
pub fn compact_to_difficulty_score(compact: u32) -> u64 {
    let exp = ((compact >> 24) & 0xFF) as u64;
    let mantissa = (compact & 0x00FF_FFFF) as u64;
    if exp == 0 || mantissa == 0 {
        return u64::MAX; // impossibly hard
    }
    // Difficulty is inversely proportional to target.
    // Encode as: (32 - exp) * 2^24 + (2^24 - mantissa)
    // This gives a monotonically increasing score as target decreases.
    let exp_component = (32u64.saturating_sub(exp)) * 0x0100_0000;
    let mantissa_component = 0x00FF_FFFF_u64.saturating_sub(mantissa);
    exp_component + mantissa_component
}

/// Convert a difficulty score back to compact target.
pub fn difficulty_score_to_compact(score: u64) -> u32 {
    let exp_component = score / 0x0100_0000;
    let mantissa_component = score % 0x0100_0000;

    let exp = 32u64.saturating_sub(exp_component);
    let mantissa = 0x00FF_FFFF_u64.saturating_sub(mantissa_component);

    // Clamp
    let exp = exp.max(1).min(32) as u32;
    let mantissa = mantissa.max(1).min(0x00FF_FFFF) as u32;

    (exp << 24) | mantissa
}

/// Compute next block difficulty using LWMA.
///
/// `timestamps[i]` = timestamp of block at height i
/// `difficulty_scores[i]` = difficulty score of block at height i
///
/// Both slices have the same length (one entry per block in the chain).
pub fn lwma_next_difficulty(
    timestamps: &[u64],
    difficulty_scores: &[u64],
) -> u32 {
    let n = timestamps.len();
    if n < 2 {
        return INITIAL_DIFFICULTY;
    }

    let window = (n - 1).min(LWMA_WINDOW); // number of intervals to look at
    let start = n - 1 - window; // index of first block in window

    let t = TARGET_BLOCK_TIME as i64;

    // Weighted sum of solve times and difficulties
    let mut weighted_solvetime_sum: i64 = 0;
    let mut difficulty_sum: u64 = 0;
    let weight_sum: i64 = (window * (window + 1) / 2) as i64;

    for i in 1..=window {
        let idx = start + i;
        let solve_time = timestamps[idx] as i64 - timestamps[idx - 1] as i64;
        // Clamp solve time to prevent timestamp manipulation
        let clamped = solve_time.max((-6) * t).min(6 * t);
        let weight = i as i64;
        weighted_solvetime_sum += clamped * weight;
        difficulty_sum += difficulty_scores[idx];
    }

    if weighted_solvetime_sum <= 0 {
        weighted_solvetime_sum = 1;
    }

    let avg_difficulty = difficulty_sum / window as u64;

    // LWMA formula:
    // next_difficulty = avg_difficulty * T * weight_sum / weighted_solvetime_sum
    //
    // If blocks are coming at exactly T seconds: weighted_solvetime_sum ≈ T * weight_sum
    // so next_difficulty ≈ avg_difficulty (no change).
    //
    // If blocks are 2x too fast: weighted_solvetime_sum ≈ (T/2) * weight_sum
    // so next_difficulty ≈ 2 * avg_difficulty (harder).

    let numerator = avg_difficulty as i128 * t as i128 * weight_sum as i128;
    let next_score = (numerator / weighted_solvetime_sum as i128).max(0) as u64;

    // Clamp: don't allow more than 2x change per block
    let clamped = next_score
        .max(avg_difficulty / 2)
        .min(avg_difficulty * 2)
        .max(1);

    let compact = difficulty_score_to_compact(clamped);

    // Never easier than MIN_DIFFICULTY
    let min_score = compact_to_difficulty_score(MIN_DIFFICULTY);
    if clamped < min_score {
        return MIN_DIFFICULTY;
    }

    compact
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hash;

    #[test]
    fn test_compact_target_roundtrip() {
        let compacts = [INITIAL_DIFFICULTY, MIN_DIFFICULTY, 0x1d00_FFFF_u32, 0x1800_1000];
        for compact in compacts {
            let target = compact_to_target(compact);
            let back = target_to_compact(&target);
            let t1 = compact_to_target(compact);
            let t2 = compact_to_target(back);
            assert_eq!(t1, t2, "roundtrip failed for {:#010x} -> {:#010x}", compact, back);
        }
    }

    #[test]
    fn test_hash_le_target() {
        let mut low = [0u8; 32]; low[1] = 0x01;
        let mut high = [0u8; 32]; high[1] = 0x02;
        assert!(hash_le_target(&low, &high));
        assert!(!hash_le_target(&high, &low));
        assert!(hash_le_target(&low, &low));
    }

    #[test]
    fn test_mine_easy() {
        let mut header = BlockHeader {
            version: 1, prev_hash: [0u8; 32], merkle_root: [0u8; 32],
            timestamp: 1000, difficulty: MIN_DIFFICULTY, nonce: 0,
        };
        assert!(mine(&mut header).is_some());
        assert!(check_pow(&header));
    }

    #[test]
    fn test_mine_initial_difficulty() {
        let mut header = BlockHeader {
            version: 1, prev_hash: [0u8; 32],
            merkle_root: hash::hash_bytes(b"test"),
            timestamp: 1000, difficulty: INITIAL_DIFFICULTY, nonce: 0,
        };
        assert!(mine(&mut header).is_some());
        assert!(check_pow(&header));
    }

    #[test]
    fn test_block_reward_halving() {
        assert_eq!(block_reward(0), 5000);
        assert_eq!(block_reward(210_000), 2500);
        assert_eq!(block_reward(420_000), 1250);
        assert_eq!(block_reward(210_000 * 64), 0);
    }

    #[test]
    fn test_difficulty_score_roundtrip() {
        let compacts = [INITIAL_DIFFICULTY, MIN_DIFFICULTY, 0x1d00_FFFF_u32];
        for c in compacts {
            let score = compact_to_difficulty_score(c);
            let back = difficulty_score_to_compact(score);
            assert_eq!(c, back, "score roundtrip failed for {:#010x}: score={}, back={:#010x}", c, score, back);
        }
    }

    #[test]
    fn test_lwma_stable_hashrate() {
        let n = 61;
        let initial_score = compact_to_difficulty_score(INITIAL_DIFFICULTY);
        let mut timestamps = Vec::new();
        let mut scores = Vec::new();
        for i in 0..n {
            timestamps.push(1_700_000_000 + i as u64 * TARGET_BLOCK_TIME);
            scores.push(initial_score);
        }
        let next = lwma_next_difficulty(&timestamps, &scores);
        let next_score = compact_to_difficulty_score(next);
        let ratio = next_score as f64 / initial_score as f64;
        assert!(ratio > 0.9 && ratio < 1.1,
            "stable: ratio={ratio}, next_score={next_score}, initial={initial_score}");
    }

    #[test]
    fn test_lwma_fast_blocks() {
        let n = 61;
        let initial_score = compact_to_difficulty_score(INITIAL_DIFFICULTY);
        let mut timestamps = Vec::new();
        let mut scores = Vec::new();
        for i in 0..n {
            timestamps.push(1_700_000_000 + i as u64 * 30); // 30s — too fast
            scores.push(initial_score);
        }
        let next = lwma_next_difficulty(&timestamps, &scores);
        let next_score = compact_to_difficulty_score(next);
        assert!(next_score > initial_score, "fast blocks should increase difficulty");
    }

    #[test]
    fn test_lwma_slow_blocks() {
        let n = 61;
        let initial_score = compact_to_difficulty_score(INITIAL_DIFFICULTY);
        let mut timestamps = Vec::new();
        let mut scores = Vec::new();
        for i in 0..n {
            timestamps.push(1_700_000_000 + i as u64 * 120); // 120s — too slow
            scores.push(initial_score);
        }
        let next = lwma_next_difficulty(&timestamps, &scores);
        let next_score = compact_to_difficulty_score(next);
        assert!(next_score < initial_score, "slow blocks should decrease difficulty");
    }

    #[test]
    fn test_lwma_few_blocks() {
        assert_eq!(lwma_next_difficulty(&[100], &[100]), INITIAL_DIFFICULTY);
    }

    #[test]
    fn test_harder_target_means_harder_mining() {
        // A smaller target (fewer leading bytes) should be harder to mine
        let easy = compact_to_target(MIN_DIFFICULTY);
        let hard = compact_to_target(INITIAL_DIFFICULTY);
        // Easy target should have more non-zero leading bytes
        assert!(easy > hard, "easy target should be larger than hard target");
    }
}
