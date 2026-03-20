/// BLAKE3 hash wrapper functions for the entire blockchain.
/// All hashing in the system goes through these functions.

pub type Hash = [u8; 32];

/// Hash arbitrary bytes with BLAKE3.
pub fn hash_bytes(data: &[u8]) -> Hash {
    *blake3::hash(data).as_bytes()
}

/// Hash two 32-byte values concatenated (for Merkle trees).
pub fn hash_pair(a: &Hash, b: &Hash) -> Hash {
    let mut input = [0u8; 64];
    input[..32].copy_from_slice(a);
    input[32..].copy_from_slice(b);
    hash_bytes(&input)
}

/// Compute Merkle root from a list of hashes.
/// If empty, returns all zeros. If one element, returns it.
/// Uses standard binary Merkle tree with BLAKE3 pair hashing.
pub fn merkle_root(hashes: &[Hash]) -> Hash {
    if hashes.is_empty() {
        return [0u8; 32];
    }
    if hashes.len() == 1 {
        return hashes[0];
    }

    let mut current: Vec<Hash> = hashes.to_vec();

    while current.len() > 1 {
        let mut next = Vec::with_capacity((current.len() + 1) / 2);
        for chunk in current.chunks(2) {
            if chunk.len() == 2 {
                next.push(hash_pair(&chunk[0], &chunk[1]));
            } else {
                // Odd element: hash with itself
                next.push(hash_pair(&chunk[0], &chunk[0]));
            }
        }
        current = next;
    }

    current[0]
}

/// Derive a key using BLAKE3's key derivation mode.
/// Context should be a globally unique string describing the purpose.
pub fn derive_key(context: &str, material: &[u8]) -> Hash {
    let mut output = [0u8; 32];
    let mut deriver = blake3::Hasher::new_derive_key(context);
    deriver.update(material);
    let mut reader = deriver.finalize_xof();
    reader.fill(&mut output);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_bytes_deterministic() {
        let a = hash_bytes(b"hello");
        let b = hash_bytes(b"hello");
        assert_eq!(a, b);
    }

    #[test]
    fn test_hash_bytes_different_inputs() {
        let a = hash_bytes(b"hello");
        let b = hash_bytes(b"world");
        assert_ne!(a, b);
    }

    #[test]
    fn test_hash_pair_order_matters() {
        let a = hash_bytes(b"a");
        let b = hash_bytes(b"b");
        assert_ne!(hash_pair(&a, &b), hash_pair(&b, &a));
    }

    #[test]
    fn test_merkle_root_empty() {
        assert_eq!(merkle_root(&[]), [0u8; 32]);
    }

    #[test]
    fn test_merkle_root_single() {
        let h = hash_bytes(b"tx1");
        assert_eq!(merkle_root(&[h]), h);
    }

    #[test]
    fn test_merkle_root_two() {
        let a = hash_bytes(b"tx1");
        let b = hash_bytes(b"tx2");
        let root = merkle_root(&[a, b]);
        assert_eq!(root, hash_pair(&a, &b));
    }

    #[test]
    fn test_merkle_root_three() {
        let a = hash_bytes(b"tx1");
        let b = hash_bytes(b"tx2");
        let c = hash_bytes(b"tx3");
        let root = merkle_root(&[a, b, c]);
        let left = hash_pair(&a, &b);
        let right = hash_pair(&c, &c); // odd element hashed with itself
        assert_eq!(root, hash_pair(&left, &right));
    }

    #[test]
    fn test_derive_key() {
        let key = derive_key("test context", b"material");
        assert_ne!(key, [0u8; 32]);
        // Deterministic
        let key2 = derive_key("test context", b"material");
        assert_eq!(key, key2);
        // Different context -> different key
        let key3 = derive_key("other context", b"material");
        assert_ne!(key, key3);
    }
}
