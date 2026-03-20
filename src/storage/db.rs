use std::collections::HashMap;
use std::path::Path;

use crate::types::block::{Block, BlockHash};
use crate::types::transaction::{OutPoint, TxOutput};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("sled error: {0}")]
    Sled(#[from] sled::Error),
    #[error("serialization error: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("not found")]
    NotFound,
}

pub struct Database {
    db: sled::Db,
    blocks: sled::Tree,
    utxos: sled::Tree,
    meta: sled::Tree,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let db = sled::open(path)?;
        let blocks = db.open_tree("blocks")?;
        let utxos = db.open_tree("utxos")?;
        let meta = db.open_tree("meta")?;
        Ok(Self { db, blocks, utxos, meta })
    }

    pub fn open_temporary() -> Result<Self, StorageError> {
        let db = sled::Config::new().temporary(true).open()?;
        let blocks = db.open_tree("blocks")?;
        let utxos = db.open_tree("utxos")?;
        let meta = db.open_tree("meta")?;
        Ok(Self { db, blocks, utxos, meta })
    }

    // -- Block storage --

    pub fn put_block(&self, hash: &BlockHash, block: &Block) -> Result<(), StorageError> {
        let data = bincode::serialize(block)?;
        self.blocks.insert(hash.as_slice(), data)?;
        Ok(())
    }

    pub fn get_block(&self, hash: &BlockHash) -> Result<Block, StorageError> {
        let data = self.blocks.get(hash)?.ok_or(StorageError::NotFound)?;
        Ok(bincode::deserialize(&data)?)
    }

    pub fn has_block(&self, hash: &BlockHash) -> bool {
        self.blocks.contains_key(hash).unwrap_or(false)
    }

    // -- UTXO storage --

    pub fn put_utxo(&self, outpoint: &OutPoint, output: &TxOutput) -> Result<(), StorageError> {
        let key = outpoint_key(outpoint);
        let data = bincode::serialize(output)?;
        self.utxos.insert(key, data)?;
        Ok(())
    }

    pub fn get_utxo(&self, outpoint: &OutPoint) -> Result<TxOutput, StorageError> {
        let key = outpoint_key(outpoint);
        let data = self.utxos.get(key)?.ok_or(StorageError::NotFound)?;
        Ok(bincode::deserialize(&data)?)
    }

    pub fn remove_utxo(&self, outpoint: &OutPoint) -> Result<(), StorageError> {
        let key = outpoint_key(outpoint);
        self.utxos.remove(key)?;
        Ok(())
    }

    /// Load all UTXOs into memory.
    pub fn load_utxo_set(&self) -> Result<HashMap<OutPoint, TxOutput>, StorageError> {
        let mut map = HashMap::new();
        for entry in self.utxos.iter() {
            let (key, value) = entry?;
            let outpoint = outpoint_from_key(&key);
            let output: TxOutput = bincode::deserialize(&value)?;
            map.insert(outpoint, output);
        }
        Ok(map)
    }

    // -- Metadata --

    pub fn set_tip(&self, hash: &BlockHash) -> Result<(), StorageError> {
        self.meta.insert("tip", hash.as_slice())?;
        Ok(())
    }

    pub fn get_tip(&self) -> Result<BlockHash, StorageError> {
        let data = self.meta.get("tip")?.ok_or(StorageError::NotFound)?;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&data);
        Ok(hash)
    }

    pub fn set_height(&self, height: u32) -> Result<(), StorageError> {
        self.meta.insert("height", &height.to_le_bytes())?;
        Ok(())
    }

    pub fn get_height(&self) -> Result<u32, StorageError> {
        let data = self.meta.get("height")?.ok_or(StorageError::NotFound)?;
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&data);
        Ok(u32::from_le_bytes(bytes))
    }

    /// Store block hash at a given height index.
    pub fn set_block_at_height(&self, height: u32, hash: &BlockHash) -> Result<(), StorageError> {
        let key = format!("h:{height}");
        self.meta.insert(key.as_bytes(), hash.as_slice())?;
        Ok(())
    }

    pub fn get_block_at_height(&self, height: u32) -> Result<BlockHash, StorageError> {
        let key = format!("h:{height}");
        let data = self.meta.get(key.as_bytes())?.ok_or(StorageError::NotFound)?;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&data);
        Ok(hash)
    }

    pub fn flush(&self) -> Result<(), StorageError> {
        self.db.flush()?;
        Ok(())
    }
}

fn outpoint_key(outpoint: &OutPoint) -> Vec<u8> {
    let mut key = Vec::with_capacity(34);
    key.extend_from_slice(&outpoint.0);
    key.extend_from_slice(&outpoint.1.to_le_bytes());
    key
}

fn outpoint_from_key(key: &[u8]) -> OutPoint {
    let mut txid = [0u8; 32];
    txid.copy_from_slice(&key[..32]);
    let index = u16::from_le_bytes([key[32], key[33]]);
    (txid, index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_storage_roundtrip() {
        let db = Database::open_temporary().unwrap();
        let block = Block {
            header: crate::types::block::BlockHeader {
                version: 1,
                prev_hash: [0u8; 32],
                merkle_root: [0u8; 32],
                timestamp: 1000,
                difficulty: 1,
                nonce: 42,
            },
            transactions: vec![],
        };
        let hash = block.hash();
        db.put_block(&hash, &block).unwrap();
        let loaded = db.get_block(&hash).unwrap();
        assert_eq!(loaded.header.nonce, 42);
    }

    #[test]
    fn test_utxo_storage() {
        let db = Database::open_temporary().unwrap();
        let outpoint = ([1u8; 32], 0u16);
        let output = TxOutput { amount: 5000, pubkey_hash: [0xABu8; 20] };
        db.put_utxo(&outpoint, &output).unwrap();

        let loaded = db.get_utxo(&outpoint).unwrap();
        assert_eq!(loaded.amount, 5000);

        db.remove_utxo(&outpoint).unwrap();
        assert!(db.get_utxo(&outpoint).is_err());
    }

    #[test]
    fn test_metadata() {
        let db = Database::open_temporary().unwrap();
        let hash = [0xFFu8; 32];
        db.set_tip(&hash).unwrap();
        db.set_height(100).unwrap();

        assert_eq!(db.get_tip().unwrap(), hash);
        assert_eq!(db.get_height().unwrap(), 100);
    }

    #[test]
    fn test_load_utxo_set() {
        let db = Database::open_temporary().unwrap();
        for i in 0..5u16 {
            let outpoint = ([i as u8; 32], i);
            let output = TxOutput { amount: (i as u64 + 1) * 1000, pubkey_hash: [i as u8; 20] };
            db.put_utxo(&outpoint, &output).unwrap();
        }
        let set = db.load_utxo_set().unwrap();
        assert_eq!(set.len(), 5);
    }
}
