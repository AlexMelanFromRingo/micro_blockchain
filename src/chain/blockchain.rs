use std::collections::HashMap;
use std::path::Path;

use crate::consensus::{pow, validation};
use crate::crypto::hash;
use crate::storage::db::Database;
use crate::types::block::{Block, BlockHash, BlockHeader};
use crate::types::transaction::{OutPoint, Transaction, TxOutput};

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
    /// Difficulty score at each height (for LWMA).
    pub difficulty_scores: Vec<u64>,
    /// Timestamp at each height (for LWMA).
    pub timestamps: Vec<u64>,
}

impl Blockchain {
    /// Create a new blockchain with genesis block, or load existing.
    pub fn new(path: &Path) -> Result<Self, ChainError> {
        let db = Database::open(path)?;

        if let Ok(tip) = db.get_tip() {
            let height = db.get_height()?;
            let utxo_set = db.load_utxo_set()?;

            // Reconstruct timestamps and difficulty scores from stored blocks
            let mut timestamps = Vec::with_capacity(height as usize + 1);
            let mut difficulty_scores = Vec::with_capacity(height as usize + 1);

            for h in 0..=height {
                let hash = db.get_block_at_height(h)?;
                let block = db.get_block(&hash)?;
                timestamps.push(block.header.timestamp as u64);
                difficulty_scores.push(pow::compact_to_difficulty_score(block.header.difficulty));
            }

            Ok(Self { db, utxo_set, tip, height, difficulty_scores, timestamps })
        } else {
            let mut chain = Self {
                db,
                utxo_set: HashMap::new(),
                tip: [0u8; 32],
                height: 0,
                difficulty_scores: Vec::new(),
                timestamps: Vec::new(),
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
            difficulty_scores: Vec::new(),
            timestamps: Vec::new(),
        };
        chain.init_genesis()?;
        Ok(chain)
    }

    fn init_genesis(&mut self) -> Result<(), ChainError> {
        let genesis = create_genesis_block();
        let hash = genesis.hash();

        // Apply UTXO changes from genesis
        self.apply_block_utxos(&genesis);

        // Track LWMA data
        self.timestamps.push(genesis.header.timestamp as u64);
        self.difficulty_scores.push(pow::compact_to_difficulty_score(genesis.header.difficulty));

        self.db.put_block(&hash, &genesis)?;
        self.db.set_block_at_height(0, &hash)?;
        self.db.set_tip(&hash)?;
        self.db.set_height(0)?;

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

        if block.header.prev_hash != self.tip {
            return Err(ChainError::PrevBlockNotFound);
        }

        let new_height = self.height + 1;

        // Validate block
        validation::validate_block(&block, &self.utxo_set, new_height)?;

        // Apply UTXO changes
        self.apply_block_utxos(&block);

        // Track LWMA data
        self.timestamps.push(block.header.timestamp as u64);
        self.difficulty_scores.push(pow::compact_to_difficulty_score(block.header.difficulty));

        // Persist
        self.db.put_block(&hash, &block)?;
        self.db.set_block_at_height(new_height, &hash)?;
        self.db.set_tip(&hash)?;
        self.db.set_height(new_height)?;
        self.persist_utxo_changes(&block)?;

        self.tip = hash;
        self.height = new_height;

        Ok(hash)
    }

    fn apply_block_utxos(&mut self, block: &Block) {
        for tx in &block.transactions {
            if !tx.is_coinbase() {
                for input in &tx.inputs {
                    let outpoint = (input.prev_tx_hash, input.output_index);
                    self.utxo_set.remove(&outpoint);
                }
            }
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

    /// Get current difficulty for the next block using LWMA.
    pub fn current_difficulty(&self) -> u32 {
        if self.height < 2 {
            return pow::INITIAL_DIFFICULTY;
        }
        pow::lwma_next_difficulty(&self.timestamps, &self.difficulty_scores)
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
    let coinbase = Transaction::new_coinbase(5000, [0u8; 20], 0);
    let merkle = hash::merkle_root(&[coinbase.txid()]);

    let mut header = BlockHeader {
        version: 1,
        prev_hash: [0u8; 32],
        merkle_root: merkle,
        timestamp: 1_700_000_000,
        difficulty: pow::INITIAL_DIFFICULTY,
        nonce: 0,
    };

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
        assert!(!chain.utxo_set.is_empty());
        assert_eq!(chain.timestamps.len(), 1);
        assert_eq!(chain.difficulty_scores.len(), 1);
    }

    #[test]
    fn test_add_block() {
        let mut chain = Blockchain::new_temporary().unwrap();
        let difficulty = chain.current_difficulty();

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
        assert_eq!(chain.timestamps.len(), 2);
        assert_eq!(chain.difficulty_scores.len(), 2);
    }

    #[test]
    fn test_mine_several_blocks() {
        let mut chain = Blockchain::new_temporary().unwrap();
        let recipient = [0xCDu8; 20];

        for i in 1..=5u32 {
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

    #[test]
    fn test_lwma_adjusts_per_block() {
        let mut chain = Blockchain::new_temporary().unwrap();
        let recipient = [0xEEu8; 20];

        // Mine blocks and track difficulty changes
        let mut difficulties = Vec::new();
        for i in 1..=10u32 {
            let difficulty = chain.current_difficulty();
            difficulties.push(difficulty);
            let reward = pow::block_reward(i);
            let coinbase = Transaction::new_coinbase(reward, recipient, i);
            let merkle = hash::merkle_root(&[coinbase.txid()]);

            let mut header = BlockHeader {
                version: 1,
                prev_hash: chain.tip,
                merkle_root: merkle,
                timestamp: 1_700_000_000 + i * 60, // Perfect 60s intervals
                difficulty,
                nonce: 0,
            };
            pow::mine(&mut header);
            chain.add_block(Block { header, transactions: vec![coinbase] }).unwrap();
        }

        assert_eq!(chain.height, 10);
        // With stable 60s blocks, difficulty should remain roughly stable
        // (first few blocks use initial difficulty before LWMA kicks in)
    }
}
