/// BIP39-like mnemonic generation using BLAKE3 instead of SHA-256.
/// Generates 12-word (128-bit) or 24-word (256-bit) mnemonics.
/// Key derivation uses BLAKE3 derive_key instead of PBKDF2-HMAC-SHA512.

use super::hash;

/// The standard BIP39 English wordlist (2048 words).
const WORDLIST: &str = include_str!("wordlist_en.txt");

fn get_wordlist() -> Vec<&'static str> {
    WORDLIST.lines().collect()
}

/// Generate a mnemonic from entropy bytes (16 or 32 bytes).
/// Uses BLAKE3 for the checksum instead of SHA-256.
pub fn entropy_to_mnemonic(entropy: &[u8]) -> Result<String, MnemonicError> {
    let ent_bits = entropy.len() * 8;
    if ent_bits != 128 && ent_bits != 256 {
        return Err(MnemonicError::InvalidEntropyLength);
    }

    let checksum_bits = ent_bits / 32;
    let checksum = hash::hash_bytes(entropy);

    // Convert entropy + checksum bits to 11-bit groups
    let mut bits = Vec::with_capacity(ent_bits + checksum_bits);
    for byte in entropy {
        for i in (0..8).rev() {
            bits.push((byte >> i) & 1);
        }
    }
    for i in (0..checksum_bits).rev() {
        let byte_idx = (8 - 1 - i) / 8; // this needs to be from MSB
        let bit_idx = 7 - (i % 8);
        bits.push((checksum[byte_idx] >> bit_idx) & 1);
    }

    // Actually let me redo the checksum bits extraction more simply
    let bits = {
        let mut b = Vec::with_capacity(ent_bits + checksum_bits);
        for byte in entropy {
            for i in (0..8).rev() {
                b.push((byte >> i) & 1);
            }
        }
        // Take first checksum_bits bits from checksum hash
        for i in 0..checksum_bits {
            let byte_idx = i / 8;
            let bit_idx = 7 - (i % 8);
            b.push((checksum[byte_idx] >> bit_idx) & 1);
        }
        b
    };

    let wordlist = get_wordlist();
    if wordlist.len() != 2048 {
        return Err(MnemonicError::InvalidWordlist);
    }

    let mut words = Vec::new();
    for chunk in bits.chunks(11) {
        let mut idx: usize = 0;
        for &bit in chunk {
            idx = (idx << 1) | (bit as usize);
        }
        words.push(wordlist[idx]);
    }

    Ok(words.join(" "))
}

/// Generate a random 12-word mnemonic.
pub fn generate_mnemonic_12() -> Result<String, MnemonicError> {
    let mut entropy = [0u8; 16];
    getrandom(&mut entropy);
    entropy_to_mnemonic(&entropy)
}

/// Generate a random 24-word mnemonic.
pub fn generate_mnemonic_24() -> Result<String, MnemonicError> {
    let mut entropy = [0u8; 32];
    getrandom(&mut entropy);
    entropy_to_mnemonic(&entropy)
}

/// Derive a 32-byte seed from a mnemonic and optional passphrase.
/// Uses BLAKE3 derive_key instead of PBKDF2-HMAC-SHA512.
pub fn mnemonic_to_seed(mnemonic: &str, passphrase: &str) -> [u8; 32] {
    let material = format!("{mnemonic}{passphrase}");
    hash::derive_key("micro_blockchain mnemonic v1", material.as_bytes())
}

/// Validate a mnemonic string.
pub fn validate_mnemonic(mnemonic: &str) -> Result<(), MnemonicError> {
    let words: Vec<&str> = mnemonic.split_whitespace().collect();
    if words.len() != 12 && words.len() != 24 {
        return Err(MnemonicError::InvalidWordCount);
    }

    let wordlist = get_wordlist();
    if wordlist.len() != 2048 {
        return Err(MnemonicError::InvalidWordlist);
    }

    // Convert words back to bits
    let mut bits = Vec::new();
    for word in &words {
        let idx = wordlist.iter().position(|w| w == word)
            .ok_or(MnemonicError::InvalidWord)?;
        for i in (0..11).rev() {
            bits.push(((idx >> i) & 1) as u8);
        }
    }

    let total_bits = words.len() * 11;
    let checksum_bits = total_bits / 33; // CS = ENT/32, and total = ENT + CS = ENT + ENT/32 = 33*ENT/32
    let ent_bits = total_bits - checksum_bits;

    // Extract entropy bytes
    let mut entropy = Vec::new();
    for chunk in bits[..ent_bits].chunks(8) {
        let mut byte = 0u8;
        for &bit in chunk {
            byte = (byte << 1) | bit;
        }
        entropy.push(byte);
    }

    // Compute expected checksum
    let checksum = hash::hash_bytes(&entropy);
    for i in 0..checksum_bits {
        let expected_bit = (checksum[i / 8] >> (7 - (i % 8))) & 1;
        if bits[ent_bits + i] != expected_bit {
            return Err(MnemonicError::InvalidChecksum);
        }
    }

    Ok(())
}

fn getrandom(buf: &mut [u8]) {
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(buf);
}

#[derive(Debug, thiserror::Error)]
pub enum MnemonicError {
    #[error("invalid entropy length, must be 16 or 32 bytes")]
    InvalidEntropyLength,
    #[error("invalid word count, must be 12 or 24")]
    InvalidWordCount,
    #[error("word not found in wordlist")]
    InvalidWord,
    #[error("invalid checksum")]
    InvalidChecksum,
    #[error("invalid wordlist")]
    InvalidWordlist,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_12_words() {
        let mnemonic = generate_mnemonic_12().unwrap();
        let words: Vec<&str> = mnemonic.split_whitespace().collect();
        assert_eq!(words.len(), 12);
    }

    #[test]
    fn test_generate_24_words() {
        let mnemonic = generate_mnemonic_24().unwrap();
        let words: Vec<&str> = mnemonic.split_whitespace().collect();
        assert_eq!(words.len(), 24);
    }

    #[test]
    fn test_mnemonic_validation_roundtrip() {
        let mnemonic = generate_mnemonic_12().unwrap();
        assert!(validate_mnemonic(&mnemonic).is_ok());
    }

    #[test]
    fn test_mnemonic_validation_24() {
        let mnemonic = generate_mnemonic_24().unwrap();
        assert!(validate_mnemonic(&mnemonic).is_ok());
    }

    #[test]
    fn test_mnemonic_invalid_word() {
        assert!(validate_mnemonic("invalid words that are not in the bip39 wordlist at all ever really truly").is_err());
    }

    #[test]
    fn test_mnemonic_to_seed_deterministic() {
        let seed1 = mnemonic_to_seed("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about", "");
        let seed2 = mnemonic_to_seed("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about", "");
        assert_eq!(seed1, seed2);
    }

    #[test]
    fn test_mnemonic_to_seed_passphrase_matters() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let seed1 = mnemonic_to_seed(mnemonic, "");
        let seed2 = mnemonic_to_seed(mnemonic, "password");
        assert_ne!(seed1, seed2);
    }

    #[test]
    fn test_entropy_to_mnemonic_deterministic() {
        let entropy = [0u8; 16];
        let m1 = entropy_to_mnemonic(&entropy).unwrap();
        let m2 = entropy_to_mnemonic(&entropy).unwrap();
        assert_eq!(m1, m2);
    }

    #[test]
    fn test_entropy_invalid_length() {
        assert!(entropy_to_mnemonic(&[0u8; 10]).is_err());
    }
}
