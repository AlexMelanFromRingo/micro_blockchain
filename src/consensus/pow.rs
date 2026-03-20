use crate::types::block::BlockHeader;

/// Check if a block header hash meets the difficulty target.
/// Difficulty is the number of leading zero bits required.
pub fn check_pow(header: &BlockHeader) -> bool {
    let h = header.hash();
    leading_zero_bits(&h) >= header.difficulty
}

/// Count leading zero bits in a hash.
pub fn leading_zero_bits(hash: &[u8; 32]) -> u32 {
    let mut count = 0u32;
    for byte in hash {
        if *byte == 0 {
            count += 8;
        } else {
            count += byte.leading_zeros();
            break;
        }
    }
    count
}

/// Mine a block header by iterating nonces.
/// Returns Some(nonce) if found within u32 range, None if exhausted.
pub fn mine(header: &mut BlockHeader) -> Option<u32> {
    for nonce in 0..=u32::MAX {
        header.nonce = nonce;
        if check_pow(header) {
            return Some(nonce);
        }
    }
    None
}

/// Compute the block reward based on height.
/// Halving every 210_000 blocks, starting at 5000 base units.
pub fn block_reward(height: u32) -> u64 {
    let halvings = height / 210_000;
    if halvings >= 64 {
        return 0;
    }
    5000u64 >> halvings
}

/// Retarget difficulty. Called every `retarget_interval` blocks.
/// Returns new difficulty (leading zero bits).
pub fn retarget(
    old_difficulty: u32,
    expected_time: u32,
    actual_time: u32,
) -> u32 {
    if actual_time == 0 {
        return old_difficulty + 1;
    }

    // If blocks came too fast, increase difficulty; too slow, decrease
    // Clamp adjustment to 4x in either direction
    let actual = actual_time.max(expected_time / 4).min(expected_time * 4);

    // new_diff adjusts based on ratio
    // If actual < expected: blocks too fast -> increase difficulty (more zeros)
    // If actual > expected: blocks too slow -> decrease difficulty (fewer zeros)
    if actual < expected_time {
        // Need harder difficulty
        (old_difficulty + 1).min(256)
    } else if actual > expected_time {
        // Need easier difficulty
        old_difficulty.saturating_sub(1).max(1)
    } else {
        old_difficulty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hash;

    #[test]
    fn test_leading_zero_bits() {
        assert_eq!(leading_zero_bits(&[0u8; 32]), 256);
        assert_eq!(leading_zero_bits(&{
            let mut h = [0u8; 32];
            h[0] = 0x80;
            h
        }), 0);
        assert_eq!(leading_zero_bits(&{
            let mut h = [0u8; 32];
            h[0] = 0x01;
            h
        }), 7);
        assert_eq!(leading_zero_bits(&{
            let mut h = [0u8; 32];
            h[1] = 0x01;
            h
        }), 15);
    }

    #[test]
    fn test_mine_low_difficulty() {
        let mut header = BlockHeader {
            version: 1,
            prev_hash: [0u8; 32],
            merkle_root: [0u8; 32],
            timestamp: 1000,
            difficulty: 1, // Just 1 leading zero bit — very easy
            nonce: 0,
        };
        let nonce = mine(&mut header);
        assert!(nonce.is_some());
        assert!(check_pow(&header));
    }

    #[test]
    fn test_mine_moderate_difficulty() {
        let mut header = BlockHeader {
            version: 1,
            prev_hash: [0u8; 32],
            merkle_root: hash::hash_bytes(b"test"),
            timestamp: 1000,
            difficulty: 8, // 8 leading zero bits = first byte must be 0
            nonce: 0,
        };
        let nonce = mine(&mut header);
        assert!(nonce.is_some());
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
    fn test_retarget() {
        // Blocks came twice as fast as expected
        let new = retarget(10, 600, 300);
        assert_eq!(new, 11); // harder

        // Blocks came twice as slow
        let new = retarget(10, 600, 1200);
        assert_eq!(new, 9); // easier

        // Can't go below 1
        let new = retarget(1, 600, 1200);
        assert_eq!(new, 1);
    }
}
