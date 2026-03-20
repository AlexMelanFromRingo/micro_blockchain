use std::collections::HashMap;

use crate::crypto::keys;
use crate::types::block::Block;
use crate::types::transaction::{OutPoint, Transaction, TxOutput};
use super::pow;

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("invalid proof of work")]
    InvalidPow,
    #[error("invalid merkle root")]
    InvalidMerkleRoot,
    #[error("no coinbase transaction")]
    NoCoinbase,
    #[error("invalid coinbase reward: got {got}, max {max}")]
    InvalidCoinbaseReward { got: u64, max: u64 },
    #[error("duplicate input: {0:?}")]
    DuplicateInput(OutPoint),
    #[error("input references unknown UTXO: {0:?}")]
    UnknownUtxo(OutPoint),
    #[error("invalid signature on input {0}")]
    InvalidSignature(usize),
    #[error("pubkey hash mismatch on input {0}")]
    PubkeyHashMismatch(usize),
    #[error("input total {input} < output total {output}")]
    InsufficientFunds { input: u64, output: u64 },
    #[error("empty block (no transactions)")]
    EmptyBlock,
    #[error("first transaction must be coinbase")]
    FirstTxNotCoinbase,
    #[error("multiple coinbase transactions")]
    MultipleCoinbase,
}

/// Validate a full block against the UTXO set.
/// `utxo_set` maps (txid, output_index) -> TxOutput.
/// `height` is the height of this block (for reward calculation).
pub fn validate_block(
    block: &Block,
    utxo_set: &HashMap<OutPoint, TxOutput>,
    height: u32,
) -> Result<(), ValidationError> {
    // 1. Check PoW
    if !pow::check_pow(&block.header) {
        return Err(ValidationError::InvalidPow);
    }

    // 2. Check Merkle root
    if !block.verify_merkle_root() {
        return Err(ValidationError::InvalidMerkleRoot);
    }

    // 3. Must have at least one transaction
    if block.transactions.is_empty() {
        return Err(ValidationError::EmptyBlock);
    }

    // 4. First tx must be coinbase, no others
    if !block.transactions[0].is_coinbase() {
        return Err(ValidationError::FirstTxNotCoinbase);
    }
    for tx in &block.transactions[1..] {
        if tx.is_coinbase() {
            return Err(ValidationError::MultipleCoinbase);
        }
    }

    // 5. Validate coinbase reward
    let max_reward = pow::block_reward(height);
    let total_fees = calculate_fees(block, utxo_set)?;
    let coinbase_output = block.transactions[0].total_output();
    if coinbase_output > max_reward + total_fees {
        return Err(ValidationError::InvalidCoinbaseReward {
            got: coinbase_output,
            max: max_reward + total_fees,
        });
    }

    // 6. Validate all non-coinbase transactions
    let mut spent_in_block: HashMap<OutPoint, ()> = HashMap::new();
    for tx in &block.transactions[1..] {
        validate_transaction(tx, utxo_set, &mut spent_in_block)?;
    }

    Ok(())
}

/// Validate a single non-coinbase transaction.
pub fn validate_transaction(
    tx: &Transaction,
    utxo_set: &HashMap<OutPoint, TxOutput>,
    spent_in_block: &mut HashMap<OutPoint, ()>,
) -> Result<u64, ValidationError> {
    let mut total_input = 0u64;

    for (i, input) in tx.inputs.iter().enumerate() {
        let outpoint: OutPoint = (input.prev_tx_hash, input.output_index);

        // Check for double-spend within block
        if spent_in_block.contains_key(&outpoint) {
            return Err(ValidationError::DuplicateInput(outpoint));
        }

        // Look up UTXO
        let utxo = utxo_set.get(&outpoint)
            .ok_or(ValidationError::UnknownUtxo(outpoint))?;

        // Verify pubkey hash matches
        let pkh = keys::pubkey_hash(&input.pubkey);
        if pkh != utxo.pubkey_hash {
            return Err(ValidationError::PubkeyHashMismatch(i));
        }

        // Verify signature
        let sighash = tx.sighash(i);
        if !keys::verify(&input.pubkey, &sighash, &input.signature) {
            return Err(ValidationError::InvalidSignature(i));
        }

        total_input += utxo.amount;
        spent_in_block.insert(outpoint, ());
    }

    let total_output = tx.total_output();
    if total_input < total_output {
        return Err(ValidationError::InsufficientFunds {
            input: total_input,
            output: total_output,
        });
    }

    Ok(total_input - total_output) // fee
}

fn calculate_fees(
    block: &Block,
    utxo_set: &HashMap<OutPoint, TxOutput>,
) -> Result<u64, ValidationError> {
    let mut total_fees = 0u64;
    for tx in &block.transactions[1..] {
        let mut tx_input_total = 0u64;
        for input in &tx.inputs {
            let outpoint: OutPoint = (input.prev_tx_hash, input.output_index);
            let utxo = utxo_set.get(&outpoint)
                .ok_or(ValidationError::UnknownUtxo(outpoint))?;
            tx_input_total += utxo.amount;
        }
        let tx_output_total = tx.total_output();
        if tx_input_total >= tx_output_total {
            total_fees += tx_input_total - tx_output_total;
        }
    }
    Ok(total_fees)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hash;
    use crate::crypto::keys;
    use crate::crypto::keys::Keypair;
    use crate::types::block::BlockHeader;
    use crate::types::transaction::{Transaction, TxInput, TxOutput};

    fn make_test_block(utxo_set: &HashMap<OutPoint, TxOutput>, height: u32) -> Block {
        let recipient = [0xABu8; 20];
        let reward = pow::block_reward(height);
        let coinbase = Transaction::new_coinbase(reward, recipient, height);
        let merkle = hash::merkle_root(&[coinbase.txid()]);

        let mut header = BlockHeader {
            version: 1,
            prev_hash: [0u8; 32],
            merkle_root: merkle,
            timestamp: 1000,
            difficulty: 1,
            nonce: 0,
        };
        pow::mine(&mut header);

        Block {
            header,
            transactions: vec![coinbase],
        }
    }

    #[test]
    fn test_validate_genesis_block() {
        let utxo_set = HashMap::new();
        let block = make_test_block(&utxo_set, 0);
        assert!(validate_block(&block, &utxo_set, 0).is_ok());
    }

    #[test]
    fn test_validate_block_bad_pow() {
        let utxo_set = HashMap::new();
        let mut block = make_test_block(&utxo_set, 0);
        block.header.difficulty = 200; // impossibly hard
        assert!(matches!(
            validate_block(&block, &utxo_set, 0),
            Err(ValidationError::InvalidPow)
        ));
    }

    #[test]
    fn test_validate_transaction_with_real_keys() {
        let kp = Keypair::generate();
        let pkh = keys::pubkey_hash(&kp.public_key());

        // Create a "previous" UTXO
        let prev_txid = hash::hash_bytes(b"prev_tx");
        let mut utxo_set = HashMap::new();
        utxo_set.insert(
            (prev_txid, 0),
            TxOutput { amount: 5000, pubkey_hash: pkh },
        );

        // Build a spending transaction
        let mut tx = Transaction {
            inputs: vec![TxInput {
                prev_tx_hash: prev_txid,
                output_index: 0,
                signature: [0u8; 64],
                pubkey: kp.public_key(),
            }],
            outputs: vec![TxOutput {
                amount: 4000,
                pubkey_hash: [0xBBu8; 20],
            }],
        };

        // Sign it
        let sighash = tx.sighash(0);
        tx.inputs[0].signature = kp.sign(&sighash);

        let mut spent = HashMap::new();
        let fee = validate_transaction(&tx, &utxo_set, &mut spent).unwrap();
        assert_eq!(fee, 1000);
    }
}
