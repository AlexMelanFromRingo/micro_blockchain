use serde::{Deserialize, Serialize};

use crate::crypto::hash::{self, Hash};
use super::transaction::Transaction;

pub type BlockHash = Hash;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub version: u8,
    pub prev_hash: BlockHash,
    pub merkle_root: Hash,
    pub timestamp: u32,
    pub difficulty: u32,
    pub nonce: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

impl BlockHeader {
    pub fn hash(&self) -> BlockHash {
        let data = bincode::serialize(self).expect("header serialization");
        hash::hash_bytes(&data)
    }
}

impl Block {
    pub fn hash(&self) -> BlockHash {
        self.header.hash()
    }

    pub fn compute_merkle_root(&self) -> Hash {
        let txids: Vec<Hash> = self.transactions.iter().map(|tx| tx.txid()).collect();
        hash::merkle_root(&txids)
    }

    pub fn verify_merkle_root(&self) -> bool {
        self.header.merkle_root == self.compute_merkle_root()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::transaction::{TxInput, TxOutput};

    fn dummy_coinbase(pubkey_hash: [u8; 20]) -> Transaction {
        Transaction {
            inputs: vec![TxInput {
                prev_tx_hash: [0u8; 32],
                output_index: 0xFFFF,
                signature: [0u8; 64],
                pubkey: [0u8; 33],
            }],
            outputs: vec![TxOutput {
                amount: 5000,
                pubkey_hash,
            }],
        }
    }

    #[test]
    fn test_block_hash_deterministic() {
        let block = Block {
            header: BlockHeader {
                version: 1,
                prev_hash: [0u8; 32],
                merkle_root: [0u8; 32],
                timestamp: 1000,
                difficulty: 1,
                nonce: 0,
            },
            transactions: vec![],
        };
        assert_eq!(block.hash(), block.hash());
    }

    #[test]
    fn test_block_hash_changes_with_nonce() {
        let mut header = BlockHeader {
            version: 1,
            prev_hash: [0u8; 32],
            merkle_root: [0u8; 32],
            timestamp: 1000,
            difficulty: 1,
            nonce: 0,
        };
        let h1 = header.hash();
        header.nonce = 1;
        let h2 = header.hash();
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_merkle_root_verification() {
        let tx = dummy_coinbase([0xABu8; 20]);
        let merkle = hash::merkle_root(&[tx.txid()]);
        let block = Block {
            header: BlockHeader {
                version: 1,
                prev_hash: [0u8; 32],
                merkle_root: merkle,
                timestamp: 1000,
                difficulty: 1,
                nonce: 0,
            },
            transactions: vec![tx],
        };
        assert!(block.verify_merkle_root());
    }
}
