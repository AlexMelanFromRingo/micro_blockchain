/// Secp256k1 key management with compressed public keys.
/// All operations use k256 (pure Rust, no_std compatible).

use k256::ecdsa::{SigningKey, VerifyingKey, Signature, signature::Signer, signature::Verifier};
use rand::rngs::OsRng;

use super::hash::Hash;

pub type CompressedPubkey = [u8; 33];
pub type SerializedSignature = [u8; 64];

/// A keypair wrapping k256's signing key.
pub struct Keypair {
    signing_key: SigningKey,
}

impl Keypair {
    /// Generate a new random keypair.
    pub fn generate() -> Self {
        Self {
            signing_key: SigningKey::random(&mut OsRng),
        }
    }

    /// Create a keypair from a 32-byte secret.
    pub fn from_bytes(secret: &[u8; 32]) -> Result<Self, k256::ecdsa::Error> {
        let signing_key = SigningKey::from_bytes(secret.into())?;
        Ok(Self { signing_key })
    }

    /// Get the secret key bytes (32 bytes).
    pub fn secret_bytes(&self) -> [u8; 32] {
        let bytes = self.signing_key.to_bytes();
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        out
    }

    /// Get the compressed public key (33 bytes, SEC1 format).
    pub fn public_key(&self) -> CompressedPubkey {
        let vk = self.signing_key.verifying_key();
        let encoded = vk.to_encoded_point(true); // compressed = true
        let bytes = encoded.as_bytes();
        let mut out = [0u8; 33];
        out.copy_from_slice(bytes);
        out
    }

    /// Sign a 32-byte message hash, returning a 64-byte compact signature (r || s).
    pub fn sign(&self, msg_hash: &Hash) -> SerializedSignature {
        let sig: Signature = self.signing_key.sign(msg_hash);
        sig.to_bytes().into()
    }
}

/// Verify a signature given a compressed public key and message hash.
pub fn verify(
    pubkey: &CompressedPubkey,
    msg_hash: &Hash,
    sig_bytes: &SerializedSignature,
) -> bool {
    let Ok(vk) = VerifyingKey::from_sec1_bytes(pubkey) else {
        return false;
    };
    let Ok(sig) = Signature::from_bytes(sig_bytes.into()) else {
        return false;
    };
    vk.verify(msg_hash, &sig).is_ok()
}

/// Compute the public key hash (20 bytes): BLAKE3(compressed_pubkey)[..20].
pub fn pubkey_hash(pubkey: &CompressedPubkey) -> [u8; 20] {
    let full = super::hash::hash_bytes(pubkey);
    let mut out = [0u8; 20];
    out.copy_from_slice(&full[..20]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hash::hash_bytes;

    #[test]
    fn test_keypair_generate() {
        let kp = Keypair::generate();
        let pk = kp.public_key();
        // Compressed key starts with 0x02 or 0x03
        assert!(pk[0] == 0x02 || pk[0] == 0x03);
    }

    #[test]
    fn test_keypair_from_bytes_roundtrip() {
        let kp = Keypair::generate();
        let secret = kp.secret_bytes();
        let kp2 = Keypair::from_bytes(&secret).unwrap();
        assert_eq!(kp.public_key(), kp2.public_key());
    }

    #[test]
    fn test_sign_and_verify() {
        let kp = Keypair::generate();
        let msg = hash_bytes(b"test message");
        let sig = kp.sign(&msg);
        assert!(verify(&kp.public_key(), &msg, &sig));
    }

    #[test]
    fn test_verify_wrong_message() {
        let kp = Keypair::generate();
        let msg = hash_bytes(b"correct message");
        let wrong_msg = hash_bytes(b"wrong message");
        let sig = kp.sign(&msg);
        assert!(!verify(&kp.public_key(), &wrong_msg, &sig));
    }

    #[test]
    fn test_verify_wrong_key() {
        let kp1 = Keypair::generate();
        let kp2 = Keypair::generate();
        let msg = hash_bytes(b"test");
        let sig = kp1.sign(&msg);
        assert!(!verify(&kp2.public_key(), &msg, &sig));
    }

    #[test]
    fn test_pubkey_hash() {
        let kp = Keypair::generate();
        let pk = kp.public_key();
        let h = pubkey_hash(&pk);
        assert_eq!(h.len(), 20);
        // Deterministic
        assert_eq!(h, pubkey_hash(&pk));
    }
}
