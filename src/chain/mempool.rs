use std::collections::HashMap;

use crate::types::transaction::{Transaction, TxId, TxOutput, OutPoint};
use crate::consensus::validation;

pub struct Mempool {
    txs: HashMap<TxId, Transaction>,
}

impl Mempool {
    pub fn new() -> Self {
        Self { txs: HashMap::new() }
    }

    /// Add a transaction to the mempool after basic validation.
    pub fn add(
        &mut self,
        tx: Transaction,
        utxo_set: &HashMap<OutPoint, TxOutput>,
    ) -> Result<TxId, MempoolError> {
        if tx.is_coinbase() {
            return Err(MempoolError::CoinbaseNotAllowed);
        }

        let txid = tx.txid();
        if self.txs.contains_key(&txid) {
            return Err(MempoolError::AlreadyExists);
        }

        // Check for double-spend against mempool
        for input in &tx.inputs {
            let outpoint = (input.prev_tx_hash, input.output_index);
            for existing in self.txs.values() {
                for ei in &existing.inputs {
                    if ei.prev_tx_hash == input.prev_tx_hash && ei.output_index == input.output_index {
                        return Err(MempoolError::DoubleSpend);
                    }
                }
            }
        }

        // Validate against UTXO set
        let mut spent = HashMap::new();
        validation::validate_transaction(&tx, utxo_set, &mut spent)
            .map_err(|e| MempoolError::Validation(e.to_string()))?;

        self.txs.insert(txid, tx);
        Ok(txid)
    }

    /// Remove a transaction by its ID.
    pub fn remove(&mut self, txid: &TxId) {
        self.txs.remove(txid);
    }

    /// Remove transactions that are included in a block.
    pub fn remove_confirmed(&mut self, txids: &[TxId]) {
        for txid in txids {
            self.txs.remove(txid);
        }
    }

    /// Get all transactions for mining (sorted by descending fee is TODO,
    /// for now just returns all).
    pub fn get_mineable(&self) -> Vec<Transaction> {
        self.txs.values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.txs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }

    pub fn contains(&self, txid: &TxId) -> bool {
        self.txs.contains_key(txid)
    }

    pub fn get(&self, txid: &TxId) -> Option<&Transaction> {
        self.txs.get(txid)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MempoolError {
    #[error("coinbase transactions not allowed in mempool")]
    CoinbaseNotAllowed,
    #[error("transaction already in mempool")]
    AlreadyExists,
    #[error("double-spend detected in mempool")]
    DoubleSpend,
    #[error("validation error: {0}")]
    Validation(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::{Keypair, pubkey_hash};
    use crate::crypto::hash;
    use crate::types::transaction::{Transaction, TxInput, TxOutput};

    fn create_funded_utxo_set() -> (Keypair, HashMap<OutPoint, TxOutput>, TxId) {
        let kp = Keypair::generate();
        let pkh = pubkey_hash(&kp.public_key());
        let prev_txid = hash::hash_bytes(b"funding_tx");
        let mut utxo_set = HashMap::new();
        utxo_set.insert(
            (prev_txid, 0),
            TxOutput { amount: 10000, pubkey_hash: pkh },
        );
        (kp, utxo_set, prev_txid)
    }

    fn create_signed_tx(kp: &Keypair, prev_txid: TxId, amount: u64) -> Transaction {
        let mut tx = Transaction {
            inputs: vec![TxInput {
                prev_tx_hash: prev_txid,
                output_index: 0,
                signature: [0u8; 64],
                pubkey: kp.public_key(),
            }],
            outputs: vec![TxOutput {
                amount,
                pubkey_hash: [0xBBu8; 20],
            }],
        };
        let sighash = tx.sighash(0);
        tx.inputs[0].signature = kp.sign(&sighash);
        tx
    }

    #[test]
    fn test_add_valid_tx() {
        let (kp, utxo_set, prev_txid) = create_funded_utxo_set();
        let tx = create_signed_tx(&kp, prev_txid, 5000);

        let mut mempool = Mempool::new();
        assert!(mempool.add(tx, &utxo_set).is_ok());
        assert_eq!(mempool.len(), 1);
    }

    #[test]
    fn test_reject_coinbase() {
        let utxo_set = HashMap::new();
        let coinbase = Transaction::new_coinbase(5000, [0u8; 20], 0);

        let mut mempool = Mempool::new();
        assert!(matches!(
            mempool.add(coinbase, &utxo_set),
            Err(MempoolError::CoinbaseNotAllowed)
        ));
    }

    #[test]
    fn test_reject_duplicate() {
        let (kp, utxo_set, prev_txid) = create_funded_utxo_set();
        let tx = create_signed_tx(&kp, prev_txid, 5000);

        let mut mempool = Mempool::new();
        mempool.add(tx.clone(), &utxo_set).unwrap();
        assert!(matches!(
            mempool.add(tx, &utxo_set),
            Err(MempoolError::AlreadyExists)
        ));
    }

    #[test]
    fn test_remove_confirmed() {
        let (kp, utxo_set, prev_txid) = create_funded_utxo_set();
        let tx = create_signed_tx(&kp, prev_txid, 5000);
        let txid = tx.txid();

        let mut mempool = Mempool::new();
        mempool.add(tx, &utxo_set).unwrap();
        mempool.remove_confirmed(&[txid]);
        assert!(mempool.is_empty());
    }
}
