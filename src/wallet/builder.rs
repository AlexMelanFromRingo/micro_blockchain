use crate::crypto::address;
use crate::types::transaction::{OutPoint, Transaction, TxInput, TxOutput};

use super::wallet::Wallet;

#[derive(Debug, thiserror::Error)]
pub enum BuilderError {
    #[error("insufficient funds: have {have}, need {need}")]
    InsufficientFunds { have: u64, need: u64 },
    #[error("invalid address: {0}")]
    InvalidAddress(String),
    #[error("no UTXOs available")]
    NoUtxos,
}

/// Build and sign a transaction from a wallet.
pub fn build_transaction(
    wallet: &Wallet,
    utxos: &[(OutPoint, TxOutput)],
    to_address: &str,
    amount: u64,
    fee: u64,
) -> Result<Transaction, BuilderError> {
    let to_pkh = address::decode_address(to_address)
        .map_err(|e| BuilderError::InvalidAddress(e.to_string()))?;

    let total_needed = amount + fee;

    // Select UTXOs (simple largest-first)
    let mut sorted_utxos = utxos.to_vec();
    sorted_utxos.sort_by(|a, b| b.1.amount.cmp(&a.1.amount));

    let mut selected = Vec::new();
    let mut selected_amount = 0u64;

    for utxo in &sorted_utxos {
        selected.push(utxo.clone());
        selected_amount += utxo.1.amount;
        if selected_amount >= total_needed {
            break;
        }
    }

    if selected_amount < total_needed {
        return Err(BuilderError::InsufficientFunds {
            have: selected_amount,
            need: total_needed,
        });
    }

    // Build inputs (signatures will be filled after)
    let inputs: Vec<TxInput> = selected
        .iter()
        .map(|(outpoint, _)| TxInput {
            prev_tx_hash: outpoint.0,
            output_index: outpoint.1,
            signature: [0u8; 64],
            pubkey: wallet.public_key(),
        })
        .collect();

    // Build outputs
    let mut outputs = vec![TxOutput {
        amount,
        pubkey_hash: to_pkh,
    }];

    // Change output
    let change = selected_amount - total_needed;
    if change > 0 {
        outputs.push(TxOutput {
            amount: change,
            pubkey_hash: wallet.pubkey_hash(),
        });
    }

    let mut tx = Transaction { inputs, outputs };

    // Sign each input
    for i in 0..tx.inputs.len() {
        let sighash = tx.sighash(i);
        tx.inputs[i].signature = wallet.sign(&sighash);
    }

    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hash;
    use crate::crypto::keys;

    #[test]
    fn test_build_transaction() {
        let wallet = Wallet::create().unwrap();
        let pkh = wallet.pubkey_hash();

        // Create fake UTXOs owned by wallet
        let txid = hash::hash_bytes(b"prev");
        let utxos = vec![
            ((txid, 0u16), TxOutput { amount: 3000, pubkey_hash: pkh }),
            ((txid, 1u16), TxOutput { amount: 4000, pubkey_hash: pkh }),
        ];

        let recipient = Wallet::create().unwrap();
        let tx = build_transaction(&wallet, &utxos, &recipient.address(), 5000, 100).unwrap();

        assert_eq!(tx.outputs[0].amount, 5000);
        assert_eq!(tx.outputs[0].pubkey_hash, recipient.pubkey_hash());
        // Change: 7000 - 5000 - 100 = 1900
        assert_eq!(tx.outputs[1].amount, 1900);
        assert_eq!(tx.outputs[1].pubkey_hash, pkh);
    }

    #[test]
    fn test_build_insufficient_funds() {
        let wallet = Wallet::create().unwrap();
        let pkh = wallet.pubkey_hash();
        let txid = hash::hash_bytes(b"prev");
        let utxos = vec![
            ((txid, 0u16), TxOutput { amount: 1000, pubkey_hash: pkh }),
        ];

        let recipient = Wallet::create().unwrap();
        let result = build_transaction(&wallet, &utxos, &recipient.address(), 5000, 100);
        assert!(matches!(result, Err(BuilderError::InsufficientFunds { .. })));
    }

    #[test]
    fn test_built_tx_signatures_valid() {
        let wallet = Wallet::create().unwrap();
        let pkh = wallet.pubkey_hash();
        let txid = hash::hash_bytes(b"prev");
        let utxos = vec![
            ((txid, 0u16), TxOutput { amount: 10000, pubkey_hash: pkh }),
        ];

        let recipient = Wallet::create().unwrap();
        let tx = build_transaction(&wallet, &utxos, &recipient.address(), 5000, 100).unwrap();

        // Verify each signature
        for (i, input) in tx.inputs.iter().enumerate() {
            let sighash = tx.sighash(i);
            assert!(keys::verify(&input.pubkey, &sighash, &input.signature));
        }
    }
}
