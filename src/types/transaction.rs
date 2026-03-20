use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

use crate::crypto::hash::{self, Hash};

pub type TxId = Hash;
pub type PubkeyHash = [u8; 20];
pub type OutPoint = (TxId, u16);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxInput {
    pub prev_tx_hash: TxId,
    pub output_index: u16,
    #[serde(with = "BigArray")]
    pub signature: [u8; 64],
    #[serde(with = "BigArray")]
    pub pubkey: [u8; 33],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxOutput {
    pub amount: u64,
    pub pubkey_hash: PubkeyHash,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
}

impl Transaction {
    /// Compute transaction ID: BLAKE3 of bincode-serialized transaction.
    pub fn txid(&self) -> TxId {
        let data = bincode::serialize(self).expect("tx serialization");
        hash::hash_bytes(&data)
    }

    /// Check if this is a coinbase transaction.
    /// Coinbase has exactly one input with prev_tx_hash all zeros and output_index 0xFFFF.
    pub fn is_coinbase(&self) -> bool {
        self.inputs.len() == 1
            && self.inputs[0].prev_tx_hash == [0u8; 32]
            && self.inputs[0].output_index == 0xFFFF
    }

    /// Create a coinbase transaction with a given reward, recipient, and block height.
    /// Height is encoded in the signature field to make each coinbase unique.
    pub fn new_coinbase(reward: u64, recipient: PubkeyHash, height: u32) -> Self {
        let mut sig = [0u8; 64];
        sig[..4].copy_from_slice(&height.to_le_bytes());
        Transaction {
            inputs: vec![TxInput {
                prev_tx_hash: [0u8; 32],
                output_index: 0xFFFF,
                signature: sig,
                pubkey: [0u8; 33],
            }],
            outputs: vec![TxOutput {
                amount: reward,
                pubkey_hash: recipient,
            }],
        }
    }

    /// Compute the "signing hash" for a given input index.
    /// This is the hash that must be signed by the input's private key.
    /// We hash the transaction with the signature field zeroed out for the target input.
    pub fn sighash(&self, input_index: usize) -> Hash {
        let mut tx_copy = self.clone();
        // Zero out all signatures (simplified sighash — sign the whole tx structure)
        for (i, input) in tx_copy.inputs.iter_mut().enumerate() {
            if i == input_index {
                input.signature = [0u8; 64];
            } else {
                input.signature = [0u8; 64];
            }
        }
        let data = bincode::serialize(&tx_copy).expect("tx serialization");
        hash::hash_bytes(&data)
    }

    /// Total output amount.
    pub fn total_output(&self) -> u64 {
        self.outputs.iter().map(|o| o.amount).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coinbase_detection() {
        let cb = Transaction::new_coinbase(5000, [0xABu8; 20], 0);
        assert!(cb.is_coinbase());
    }

    #[test]
    fn test_regular_tx_not_coinbase() {
        let tx = Transaction {
            inputs: vec![TxInput {
                prev_tx_hash: [1u8; 32],
                output_index: 0,
                signature: [0u8; 64],
                pubkey: [0u8; 33],
            }],
            outputs: vec![TxOutput {
                amount: 1000,
                pubkey_hash: [0xABu8; 20],
            }],
        };
        assert!(!tx.is_coinbase());
    }

    #[test]
    fn test_txid_deterministic() {
        let tx = Transaction::new_coinbase(5000, [0xABu8; 20], 0);
        assert_eq!(tx.txid(), tx.txid());
    }

    #[test]
    fn test_sighash_zeroes_signatures() {
        let tx = Transaction {
            inputs: vec![TxInput {
                prev_tx_hash: [1u8; 32],
                output_index: 0,
                signature: [0xFFu8; 64],
                pubkey: [2u8; 33],
            }],
            outputs: vec![TxOutput {
                amount: 1000,
                pubkey_hash: [3u8; 20],
            }],
        };
        let h1 = tx.sighash(0);
        // A different tx with same structure but different sig should have same sighash
        let mut tx2 = tx.clone();
        tx2.inputs[0].signature = [0xAAu8; 64];
        assert_eq!(h1, tx2.sighash(0));
    }

    #[test]
    fn test_total_output() {
        let tx = Transaction {
            inputs: vec![],
            outputs: vec![
                TxOutput { amount: 1000, pubkey_hash: [0u8; 20] },
                TxOutput { amount: 2000, pubkey_hash: [0u8; 20] },
            ],
        };
        assert_eq!(tx.total_output(), 3000);
    }
}
