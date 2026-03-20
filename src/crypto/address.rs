/// Bech32m address encoding/decoding with HRP "mc" (micro-chain).
/// Address payload is the 20-byte BLAKE3 hash of the compressed public key.

use bech32::{Bech32m, Hrp};

use super::keys::{CompressedPubkey, pubkey_hash};

pub type PubkeyHash = [u8; 20];

const HRP: Hrp = Hrp::parse_unchecked("mc");

/// Encode a compressed public key into a Bech32m address.
pub fn encode_address(pubkey: &CompressedPubkey) -> String {
    let hash = pubkey_hash(pubkey);
    encode_address_from_hash(&hash)
}

/// Encode a 20-byte pubkey hash into a Bech32m address.
pub fn encode_address_from_hash(hash: &PubkeyHash) -> String {
    bech32::encode::<Bech32m>(HRP, hash).expect("valid bech32m encoding")
}

/// Decode a Bech32m address into a 20-byte pubkey hash.
pub fn decode_address(address: &str) -> Result<PubkeyHash, AddressError> {
    let (hrp, data) = bech32::decode(address).map_err(|_| AddressError::InvalidBech32)?;
    if hrp != HRP {
        return Err(AddressError::WrongHrp);
    }
    if data.len() != 20 {
        return Err(AddressError::InvalidLength);
    }
    let mut hash = [0u8; 20];
    hash.copy_from_slice(&data);
    Ok(hash)
}

#[derive(Debug, thiserror::Error)]
pub enum AddressError {
    #[error("invalid bech32 encoding")]
    InvalidBech32,
    #[error("wrong HRP, expected 'mc'")]
    WrongHrp,
    #[error("invalid payload length, expected 20 bytes")]
    InvalidLength,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::Keypair;

    #[test]
    fn test_address_roundtrip() {
        let kp = Keypair::generate();
        let addr = encode_address(&kp.public_key());
        assert!(addr.starts_with("mc1"));
        let decoded = decode_address(&addr).unwrap();
        assert_eq!(decoded, pubkey_hash(&kp.public_key()));
    }

    #[test]
    fn test_address_invalid() {
        assert!(decode_address("invalid").is_err());
        assert!(decode_address("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4").is_err());
    }

    #[test]
    fn test_address_from_hash() {
        let hash = [0xABu8; 20];
        let addr = encode_address_from_hash(&hash);
        let decoded = decode_address(&addr).unwrap();
        assert_eq!(decoded, hash);
    }
}
