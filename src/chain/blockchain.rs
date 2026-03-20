use std::collections::HashMap;
use std::path::Path;

use crate::consensus::{pow, validation};
use crate::crypto::hash;
use crate::storage::db::Database;
use crate::types::block::{Block, BlockHash, BlockHeader};
use crate::types::transaction::{OutPoint, Transaction, TxOutput};

/// Retarget every 100 blocks (small for a micro-chain).
pub const RETARGET_INTERVAL: u32 = 100;
/// Target 60 seconds per block.
pub const TARGET_BLOCK_TIME: u32 = 60;
/// Initial difficulty: 8 leading zero bits.
pub const INITIAL_DIFFICULTY: u32 = 8;

#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("storage error: {0}")]
    Storage(#[from] crate::storage::db::StorageError),
    #[error("validation error: {0}")]
    Validation(#[from] validation::ValidationError),
    #[error("block already exists")]
    BlockExists,
    #[error("previous block not found")]
    PrevBlockNotFound,
}

pub struct Blockchain {
    pub db: Database,
    pub utxo_set: HashMap<OutPoint, TxOutput>,
    pub tip: BlockHash,
    pub height: u32,
}

impl Blockchain {
    /// Create a new blockchain with genesis block, or load existing.
    pub fn new(path: &Path) -> Result<Self, ChainError> {
        let db = Database::open(path)?;

        if let Ok(tip) = db.get_tip() {
            let height = db.get_height()?;
            let utxo_set = db.load_utxo_set()?;
            Ok(Self { db, utxo_set, tip, height })
        } else {
            let mut chain = Self {
                db,
                utxo_set: HashMap::new(),
                tip: [0u8; 32],
                height: 0,
            };
            chain.init_genesis()?;
            Ok(chain)
        }
    }

    /// Create with temporary (in-memory) storage for testing.
    pub fn new_temporary() -> Result<Self, ChainError> {
        let db = Database::open_temporary()?;
        let mut chain = Self {
            db,
            utxo_set: HashMap::new(),
            tip: [0u8; 32],
            height: 0,
        };
        chain.init_genesis()?;
        Ok(chain)
    }

    fn init_genesis(&mut self) -> Result<(), ChainError> {
        let genesis = create_genesis_block();
        let hash = genesis.hash();

        // Apply UTXO changes from genesis
        self.apply_block_utxos(&genesis);

        self.db.put_block(&hash, &genesis)?;
        self.db.set_block_at_height(0, &hash)?;
        self.db.set_tip(&hash)?;
        self.db.set_height(0)?;

        // Persist UTXOs
        for (outpoint, output) in &self.utxo_set {
            self.db.put_utxo(outpoint, output)?;
        }

        self.tip = hash;
        self.height = 0;
        Ok(())
    }

    /// Add a new block to the chain.
    pub fn add_block(&mut self, block: Block) -> Result<BlockHash, ChainError> {
        let hash = block.hash();

        if self.db.has_block(&hash) {
            return Err(ChainError::BlockExists);
        }

        // Check previous block is our current tip
        if block.header.prev_hash != self.tip {
            return Err(ChainError::PrevBlockNotFound);
        }

        let new_height = self.height + 1;

        // Validate block
        validation::validate_block(&block, &self.utxo_set, new_height)?;

        // Apply UTXO changes
        self.apply_block_utxos(&block);

        // Persist
        self.db.put_block(&hash, &block)?;
        self.db.set_block_at_height(new_height, &hash)?;
        self.db.set_tip(&hash)?;
        self.db.set_height(new_height)?;

        // Update UTXO persistence (simplified: we trust in-memory set + sled)
        self.persist_utxo_changes(&block)?;

        self.tip = hash;
        self.height = new_height;

        Ok(hash)
    }

    fn apply_block_utxos(&mut self, block: &Block) {
        for tx in &block.transactions {
            // Remove spent UTXOs (skip coinbase inputs)
            if !tx.is_coinbase() {
                for input in &tx.inputs {
                    let outpoint = (input.prev_tx_hash, input.output_index);
                    self.utxo_set.remove(&outpoint);
                }
            }
            // Add new UTXOs
            let txid = tx.txid();
            for (i, output) in tx.outputs.iter().enumerate() {
                self.utxo_set.insert((txid, i as u16), output.clone());
            }
        }
    }

    fn persist_utxo_changes(&self, block: &Block) -> Result<(), ChainError> {
        for tx in &block.transactions {
            if !tx.is_coinbase() {
                for input in &tx.inputs {
                    let outpoint = (input.prev_tx_hash, input.output_index);
                    let _ = self.db.remove_utxo(&outpoint);
                }
            }
            let txid = tx.txid();
            for (i, output) in tx.outputs.iter().enumerate() {
                self.db.put_utxo(&(txid, i as u16), output)?;
            }
        }
        Ok(())
    }

    /// Get current difficulty for next block.
    pub fn current_difficulty(&self) -> u32 {
        if self.height == 0 || self.height % RETARGET_INTERVAL != 0 {
            if let Ok(block) = self.db.get_block(&self.tip) {
                return block.header.difficulty;
            }
            return INITIAL_DIFFICULTY;
        }
        // Retarget
        self.calculate_new_difficulty()
    }

    fn calculate_new_difficulty(&self) -> u32 {
        let current_block = self.db.get_block(&self.tip).unwrap();
        let start_height = self.height.saturating_sub(RETARGET_INTERVAL);
        let start_hash = self.db.get_block_at_height(start_height).unwrap();
        let start_block = self.db.get_block(&start_hash).unwrap();

        let actual_time = current_block.header.timestamp - start_block.header.timestamp;
        let expected_time = RETARGET_INTERVAL * TARGET_BLOCK_TIME;

        pow::retarget(current_block.header.difficulty, expected_time, actual_time)
    }

    /// Get UTXOs belonging to a specific pubkey hash.
    pub fn get_utxos_for(&self, pubkey_hash: &[u8; 20]) -> Vec<(OutPoint, TxOutput)> {
        self.utxo_set
            .iter()
            .filter(|(_, output)| &output.pubkey_hash == pubkey_hash)
            .map(|(op, out)| (*op, out.clone()))
            .collect()
    }

    /// Get balance for a pubkey hash.
    pub fn get_balance(&self, pubkey_hash: &[u8; 20]) -> u64 {
        self.get_utxos_for(pubkey_hash)
            .iter()
            .map(|(_, out)| out.amount)
            .sum()
    }

    pub fn get_block(&self, hash: &BlockHash) -> Result<Block, ChainError> {
        Ok(self.db.get_block(hash)?)
    }

    pub fn get_block_at_height(&self, height: u32) -> Result<Block, ChainError> {
        let hash = self.db.get_block_at_height(height)?;
        Ok(self.db.get_block(&hash)?)
    }
}

/// Create the genesis block with a known coinbase.
pub fn create_genesis_block() -> Block {
    // Genesis coinbase pays to a "burn" address (all zeros)
    let coinbase = Transaction::new_coinbase(5000, [0u8; 20], 0);
    let merkle = hash::merkle_root(&[coinbase.txid()]);

    let mut header = BlockHeader {
        version: 1,
        prev_hash: [0u8; 32],
        merkle_root: merkle,
        timestamp: 1_700_000_000, // Fixed genesis timestamp
        difficulty: INITIAL_DIFFICULTY,
        nonce: 0,
    };

    // Mine genesis
    pow::mine(&mut header);

    Block {
        header,
        transactions: vec![coinbase],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_block_valid() {
        let chain = Blockchain::new_temporary().unwrap();
        assert_eq!(chain.height, 0);
        assert_ne!(chain.tip, [0u8; 32]);
        // Genesis UTXO exists
        assert!(!chain.utxo_set.is_empty());
    }

    #[test]
    fn test_add_block() {
        let mut chain = Blockchain::new_temporary().unwrap();
        let difficulty = chain.current_difficulty();

        // Mine a new block
        let recipient = [0xABu8; 20];
        let reward = pow::block_reward(1);
        let coinbase = Transaction::new_coinbase(reward, recipient, 1);
        let merkle = hash::merkle_root(&[coinbase.txid()]);

        let mut header = BlockHeader {
            version: 1,
            prev_hash: chain.tip,
            merkle_root: merkle,
            timestamp: 1_700_000_060,
            difficulty,
            nonce: 0,
        };
        pow::mine(&mut header);

        let block = Block { header, transactions: vec![coinbase] };
        chain.add_block(block).unwrap();

        assert_eq!(chain.height, 1);
        assert_eq!(chain.get_balance(&recipient), reward);
    }

    #[test]
    fn test_mine_several_blocks() {
        let mut chain = Blockchain::new_temporary().unwrap();
        let recipient = [0xCDu8; 20];

        for i in 1..=5 {
            let difficulty = chain.current_difficulty();
            let reward = pow::block_reward(i);
            let coinbase = Transaction::new_coinbase(reward, recipient, i);
            let merkle = hash::merkle_root(&[coinbase.txid()]);

            let mut header = BlockHeader {
                version: 1,
                prev_hash: chain.tip,
                merkle_root: merkle,
                timestamp: 1_700_000_000 + i * 60,
                difficulty,
                nonce: 0,
            };
            pow::mine(&mut header);

            let block = Block { header, transactions: vec![coinbase] };
            chain.add_block(block).unwrap();
        }

        assert_eq!(chain.height, 5);
        assert_eq!(chain.get_balance(&recipient), 5000 * 5);
    }
}
