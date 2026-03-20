use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use micro_blockchain::chain::blockchain::Blockchain;
use micro_blockchain::chain::mempool::Mempool;
use micro_blockchain::consensus::pow;
use micro_blockchain::crypto::address;
use micro_blockchain::crypto::hash;
use micro_blockchain::crypto::keys;
use micro_blockchain::crypto::mnemonic;
use micro_blockchain::network::protocol::Message;
use micro_blockchain::network::server::Server;
use micro_blockchain::network::sync_manager::{NodeEvent, SyncManager};
use micro_blockchain::types::block::{Block, BlockHeader};
use micro_blockchain::types::transaction::{Transaction, TxInput, TxOutput};
use micro_blockchain::wallet::builder;
use micro_blockchain::wallet::wallet::Wallet;

use tokio::sync::{mpsc, RwLock};

// ============================================================
// Wallet Integration Tests
// ============================================================

#[test]
fn test_full_wallet_lifecycle() {
    // 1. Create wallet from mnemonic
    let wallet = Wallet::create().unwrap();
    let mnemonic_phrase = wallet.mnemonic().unwrap().to_string();
    let addr = wallet.address();

    // 2. Verify address is valid
    assert!(addr.starts_with("mc1"));
    let pkh = address::decode_address(&addr).unwrap();
    assert_eq!(pkh, wallet.pubkey_hash());

    // 3. Restore from mnemonic — same keys
    let restored = Wallet::from_mnemonic(&mnemonic_phrase, "").unwrap();
    assert_eq!(wallet.address(), restored.address());
    assert_eq!(wallet.public_key(), restored.public_key());

    // 4. Different passphrase — different keys
    let different = Wallet::from_mnemonic(&mnemonic_phrase, "secret").unwrap();
    assert_ne!(wallet.address(), different.address());

    // 5. Sign and verify
    let msg = hash::hash_bytes(b"integration test");
    let sig = wallet.sign(&msg);
    assert!(keys::verify(&wallet.public_key(), &msg, &sig));
}

#[test]
fn test_mnemonic_12_and_24_words() {
    let m12 = mnemonic::generate_mnemonic_12().unwrap();
    assert_eq!(m12.split_whitespace().count(), 12);
    assert!(mnemonic::validate_mnemonic(&m12).is_ok());

    let m24 = mnemonic::generate_mnemonic_24().unwrap();
    assert_eq!(m24.split_whitespace().count(), 24);
    assert!(mnemonic::validate_mnemonic(&m24).is_ok());
}

#[test]
fn test_address_encoding_decoding() {
    for _ in 0..10 {
        let wallet = Wallet::create().unwrap();
        let addr = wallet.address();
        assert!(addr.starts_with("mc1"));
        let decoded = address::decode_address(&addr).unwrap();
        assert_eq!(decoded, wallet.pubkey_hash());
    }
}

// ============================================================
// Blockchain + Mining Integration Tests
// ============================================================

#[test]
fn test_mine_and_spend() {
    let mut chain = Blockchain::new_temporary().unwrap();
    let miner_wallet = Wallet::create().unwrap();
    let recipient_wallet = Wallet::create().unwrap();

    // Mine 3 blocks to miner
    for i in 1..=3 {
        let height = chain.height + 1;
        let difficulty = chain.current_difficulty();
        let reward = pow::block_reward(height);
        let coinbase = Transaction::new_coinbase(reward, miner_wallet.pubkey_hash(), height);
        let merkle = hash::merkle_root(&[coinbase.txid()]);

        let mut header = BlockHeader {
            version: 1,
            prev_hash: chain.tip,
            merkle_root: merkle,
            timestamp: 1_700_000_000 + height * 60,
            difficulty,
            nonce: 0,
        };
        pow::mine(&mut header);
        chain.add_block(Block { header, transactions: vec![coinbase] }).unwrap();
    }

    assert_eq!(chain.height, 3);
    assert_eq!(chain.get_balance(&miner_wallet.pubkey_hash()), 5000 * 3);

    // Spend: send 2000 from miner to recipient
    let utxos = chain.get_utxos_for(&miner_wallet.pubkey_hash());
    let tx = builder::build_transaction(
        &miner_wallet,
        &utxos,
        &recipient_wallet.address(),
        2000,
        100,
    ).unwrap();

    // Verify tx signatures
    for (i, input) in tx.inputs.iter().enumerate() {
        let sighash = tx.sighash(i);
        assert!(keys::verify(&input.pubkey, &sighash, &input.signature));
    }

    // Mine a block containing this transaction
    let height = chain.height + 1;
    let reward = pow::block_reward(height);
    let coinbase = Transaction::new_coinbase(reward, miner_wallet.pubkey_hash(), height);
    let all_txs = vec![coinbase, tx.clone()];
    let txids: Vec<_> = all_txs.iter().map(|t| t.txid()).collect();
    let merkle = hash::merkle_root(&txids);

    let mut header = BlockHeader {
        version: 1,
        prev_hash: chain.tip,
        merkle_root: merkle,
        timestamp: 1_700_000_000 + height * 60,
        difficulty: chain.current_difficulty(),
        nonce: 0,
    };
    pow::mine(&mut header);
    chain.add_block(Block { header, transactions: all_txs }).unwrap();

    // Verify balances after transfer
    let miner_balance = chain.get_balance(&miner_wallet.pubkey_hash());
    let recipient_balance = chain.get_balance(&recipient_wallet.pubkey_hash());

    assert_eq!(recipient_balance, 2000);
    // Miner: 3 * 5000 (previous blocks) + 5000 (new block reward) + change - what was spent
    // Spent: 5000 (one UTXO) -> 2000 to recipient + 100 fee + 2900 change
    // So miner has: 2 * 5000 (untouched) + 5000 (new reward) + 2900 (change) = 17900
    assert_eq!(miner_balance, 17900);
}

#[test]
fn test_chain_rejects_invalid_block() {
    let mut chain = Blockchain::new_temporary().unwrap();

    // Try adding a block with wrong prev_hash
    let coinbase = Transaction::new_coinbase(5000, [0xAA; 20], 1);
    let merkle = hash::merkle_root(&[coinbase.txid()]);
    let header = BlockHeader {
        version: 1,
        prev_hash: [0xFF; 32], // wrong
        merkle_root: merkle,
        timestamp: 1_700_001_000,
        difficulty: chain.current_difficulty(),
        nonce: 0,
    };
    let result = chain.add_block(Block { header, transactions: vec![coinbase] });
    assert!(result.is_err());
}

#[test]
fn test_chain_rejects_double_spend() {
    let mut chain = Blockchain::new_temporary().unwrap();
    let wallet = Wallet::create().unwrap();

    // Mine a block to get funds
    let height = 1;
    let coinbase = Transaction::new_coinbase(5000, wallet.pubkey_hash(), height);
    let merkle = hash::merkle_root(&[coinbase.txid()]);
    let mut header = BlockHeader {
        version: 1,
        prev_hash: chain.tip,
        merkle_root: merkle,
        timestamp: 1_700_000_060,
        difficulty: chain.current_difficulty(),
        nonce: 0,
    };
    pow::mine(&mut header);
    chain.add_block(Block { header, transactions: vec![coinbase.clone()] }).unwrap();

    // Create two transactions spending the same UTXO
    let utxos = chain.get_utxos_for(&wallet.pubkey_hash());
    let recipient1 = Wallet::create().unwrap();
    let recipient2 = Wallet::create().unwrap();

    let tx1 = builder::build_transaction(&wallet, &utxos, &recipient1.address(), 2000, 100).unwrap();
    let tx2 = builder::build_transaction(&wallet, &utxos, &recipient2.address(), 2000, 100).unwrap();

    // Try to mine a block with both (double spend)
    let height = 2;
    let cb = Transaction::new_coinbase(5000, wallet.pubkey_hash(), height);
    let all_txs = vec![cb, tx1, tx2];
    let txids: Vec<_> = all_txs.iter().map(|t| t.txid()).collect();
    let merkle = hash::merkle_root(&txids);

    let mut header = BlockHeader {
        version: 1,
        prev_hash: chain.tip,
        merkle_root: merkle,
        timestamp: 1_700_000_120,
        difficulty: chain.current_difficulty(),
        nonce: 0,
    };
    pow::mine(&mut header);

    let result = chain.add_block(Block { header, transactions: all_txs });
    assert!(result.is_err()); // Should reject double-spend
}

#[test]
fn test_chain_rejects_overspend() {
    let mut chain = Blockchain::new_temporary().unwrap();
    let wallet = Wallet::create().unwrap();

    // Mine to get 5000
    let height = 1;
    let coinbase = Transaction::new_coinbase(5000, wallet.pubkey_hash(), height);
    let merkle = hash::merkle_root(&[coinbase.txid()]);
    let mut header = BlockHeader {
        version: 1,
        prev_hash: chain.tip,
        merkle_root: merkle,
        timestamp: 1_700_000_060,
        difficulty: chain.current_difficulty(),
        nonce: 0,
    };
    pow::mine(&mut header);
    chain.add_block(Block { header, transactions: vec![coinbase] }).unwrap();

    // Try to spend more than we have
    let utxos = chain.get_utxos_for(&wallet.pubkey_hash());
    let recipient = Wallet::create().unwrap();
    let result = builder::build_transaction(&wallet, &utxos, &recipient.address(), 10000, 100);
    assert!(result.is_err()); // Insufficient funds
}

// ============================================================
// Mempool Integration Tests
// ============================================================

#[test]
fn test_mempool_accepts_valid_rejects_invalid() {
    let mut chain = Blockchain::new_temporary().unwrap();
    let wallet = Wallet::create().unwrap();

    // Mine to get funds
    let height = 1;
    let coinbase = Transaction::new_coinbase(5000, wallet.pubkey_hash(), height);
    let merkle = hash::merkle_root(&[coinbase.txid()]);
    let mut header = BlockHeader {
        version: 1,
        prev_hash: chain.tip,
        merkle_root: merkle,
        timestamp: 1_700_000_060,
        difficulty: chain.current_difficulty(),
        nonce: 0,
    };
    pow::mine(&mut header);
    chain.add_block(Block { header, transactions: vec![coinbase] }).unwrap();

    let mut mempool = Mempool::new();

    // Valid tx
    let utxos = chain.get_utxos_for(&wallet.pubkey_hash());
    let recipient = Wallet::create().unwrap();
    let tx = builder::build_transaction(&wallet, &utxos, &recipient.address(), 2000, 100).unwrap();
    assert!(mempool.add(tx.clone(), &chain.utxo_set).is_ok());
    assert_eq!(mempool.len(), 1);

    // Duplicate rejected
    assert!(mempool.add(tx, &chain.utxo_set).is_err());

    // Coinbase rejected
    let cb = Transaction::new_coinbase(5000, [0u8; 20], 0);
    assert!(mempool.add(cb, &chain.utxo_set).is_err());
}

// ============================================================
// P2P Networking Integration Tests
// ============================================================

#[tokio::test]
async fn test_two_nodes_sync() {
    // Node 1: mine some blocks
    let chain1 = Blockchain::new_temporary().unwrap();
    let chain1 = Arc::new(RwLock::new(chain1));
    let mempool1 = Arc::new(RwLock::new(Mempool::new()));
    let (event_tx1, mut event_rx1) = mpsc::unbounded_channel();

    let miner = Wallet::create().unwrap();

    // Mine 5 blocks on node 1
    {
        let mut chain = chain1.write().await;
        for _ in 0..5 {
            let height = chain.height + 1;
            let difficulty = chain.current_difficulty();
            let reward = pow::block_reward(height);
            let coinbase = Transaction::new_coinbase(reward, miner.pubkey_hash(), height);
            let merkle = hash::merkle_root(&[coinbase.txid()]);
            let mut header = BlockHeader {
                version: 1,
                prev_hash: chain.tip,
                merkle_root: merkle,
                timestamp: 1_700_000_000 + height * 60,
                difficulty,
                nonce: 0,
            };
            pow::mine(&mut header);
            chain.add_block(Block { header, transactions: vec![coinbase] }).unwrap();
        }
        assert_eq!(chain.height, 5);
    }

    // Start node 1 server
    let sync1 = Arc::new(SyncManager::new(
        chain1.clone(),
        mempool1.clone(),
        18333,
        event_tx1,
    ));
    let server1 = Server::new("127.0.0.1:18333".parse().unwrap(), sync1.clone());
    tokio::spawn(async move {
        let _ = server1.run().await;
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Node 2: fresh chain, connect to node 1
    let chain2 = Blockchain::new_temporary().unwrap();
    let initial_height2 = chain2.height;
    let chain2 = Arc::new(RwLock::new(chain2));
    let mempool2 = Arc::new(RwLock::new(Mempool::new()));
    let (event_tx2, mut event_rx2) = mpsc::unbounded_channel();

    let sync2 = Arc::new(SyncManager::new(
        chain2.clone(),
        mempool2.clone(),
        18334,
        event_tx2,
    ));

    // Connect node 2 to node 1
    let sync2_clone = sync2.clone();
    let connect_handle = tokio::spawn(async move {
        sync2_clone.connect_to_peer("127.0.0.1:18333".parse().unwrap()).await
    });

    // Wait for sync (with timeout)
    let timeout = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => {
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                let chain = chain2.read().await;
                if chain.height >= 5 {
                    break;
                }
            }
        }
    }

    // Verify node 2 synced
    let chain = chain2.read().await;
    assert_eq!(chain.height, 5, "Node 2 should have synced to height 5");
    assert_eq!(chain.get_balance(&miner.pubkey_hash()), 5000 * 5);
}

#[tokio::test]
async fn test_transaction_propagation() {
    // Set up node with funded wallet
    let chain = Blockchain::new_temporary().unwrap();
    let chain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new()));
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<NodeEvent>();

    let wallet = Wallet::create().unwrap();

    // Mine to fund wallet
    {
        let mut c = chain.write().await;
        let height = 1;
        let coinbase = Transaction::new_coinbase(5000, wallet.pubkey_hash(), height);
        let merkle = hash::merkle_root(&[coinbase.txid()]);
        let mut header = BlockHeader {
            version: 1,
            prev_hash: c.tip,
            merkle_root: merkle,
            timestamp: 1_700_000_060,
            difficulty: c.current_difficulty(),
            nonce: 0,
        };
        pow::mine(&mut header);
        c.add_block(Block { header, transactions: vec![coinbase] }).unwrap();
    }

    // Build a transaction
    let recipient = Wallet::create().unwrap();
    let utxos = {
        let c = chain.read().await;
        c.get_utxos_for(&wallet.pubkey_hash())
    };
    let tx = builder::build_transaction(&wallet, &utxos, &recipient.address(), 1000, 50).unwrap();
    let txid = tx.txid();

    // Add to mempool
    {
        let c = chain.read().await;
        let mut mp = mempool.write().await;
        mp.add(tx.clone(), &c.utxo_set).unwrap();
    }

    assert_eq!(mempool.read().await.len(), 1);

    // Mine block with mempool tx
    {
        let mut c = chain.write().await;
        let height = c.height + 1;
        let reward = pow::block_reward(height);
        let coinbase = Transaction::new_coinbase(reward, wallet.pubkey_hash(), height);
        let mempool_txs = mempool.read().await.get_mineable();
        let mut all_txs = vec![coinbase];
        all_txs.extend(mempool_txs);
        let txids: Vec<_> = all_txs.iter().map(|t| t.txid()).collect();
        let merkle = hash::merkle_root(&txids);

        let mut header = BlockHeader {
            version: 1,
            prev_hash: c.tip,
            merkle_root: merkle,
            timestamp: 1_700_000_120,
            difficulty: c.current_difficulty(),
            nonce: 0,
        };
        pow::mine(&mut header);
        c.add_block(Block { header, transactions: all_txs }).unwrap();

        // Clear mempool
        mempool.write().await.remove_confirmed(&[txid]);
    }

    // Verify final state
    let c = chain.read().await;
    assert_eq!(c.get_balance(&recipient.pubkey_hash()), 1000);
    assert_eq!(mempool.read().await.len(), 0);
}

// ============================================================
// Protocol Message Tests
// ============================================================

#[test]
fn test_all_message_types_serialize() {
    let messages = vec![
        Message::Version { height: 100, best_hash: [0xAB; 32], listen_port: 8333 },
        Message::VerAck,
        Message::GetBlocks { from_hash: [0; 32], count: 500 },
        Message::Blocks(vec![]),
        Message::NewTx(Transaction::new_coinbase(5000, [0; 20], 0)),
        Message::GetPeers,
        Message::Peers(vec!["127.0.0.1:8333".parse().unwrap()]),
        Message::Ping(42),
        Message::Pong(42),
    ];

    for msg in messages {
        let bytes = msg.to_bytes();
        assert!(bytes.len() > 4); // at least length prefix
        let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let payload = &bytes[4..];
        assert_eq!(payload.len(), len as usize);
        let decoded = Message::from_bytes(payload).unwrap();
        // Just verify it doesn't panic
        let _ = format!("{:?}", decoded);
    }
}

// ============================================================
// Stress Tests
// ============================================================

#[test]
fn test_mine_100_blocks() {
    let mut chain = Blockchain::new_temporary().unwrap();
    let wallet = Wallet::create().unwrap();

    for _ in 0..100 {
        let height = chain.height + 1;
        let difficulty = chain.current_difficulty();
        let reward = pow::block_reward(height);
        let coinbase = Transaction::new_coinbase(reward, wallet.pubkey_hash(), height);
        let merkle = hash::merkle_root(&[coinbase.txid()]);

        let mut header = BlockHeader {
            version: 1,
            prev_hash: chain.tip,
            merkle_root: merkle,
            timestamp: 1_700_000_000 + height * 60,
            difficulty,
            nonce: 0,
        };
        pow::mine(&mut header);
        chain.add_block(Block { header, transactions: vec![coinbase] }).unwrap();
    }

    assert_eq!(chain.height, 100);
    assert_eq!(chain.get_balance(&wallet.pubkey_hash()), 5000 * 100);
    assert_eq!(chain.utxo_set.len(), 101); // 100 miner UTXOs + 1 genesis
}

#[test]
fn test_chain_of_transfers() {
    let mut chain = Blockchain::new_temporary().unwrap();

    // Create 5 wallets, mine to first, then chain transfers
    let wallets: Vec<Wallet> = (0..5).map(|_| Wallet::create().unwrap()).collect();

    // Mine to wallet[0]
    let height = 1;
    let coinbase = Transaction::new_coinbase(5000, wallets[0].pubkey_hash(), height);
    let merkle = hash::merkle_root(&[coinbase.txid()]);
    let mut header = BlockHeader {
        version: 1,
        prev_hash: chain.tip,
        merkle_root: merkle,
        timestamp: 1_700_000_060,
        difficulty: chain.current_difficulty(),
        nonce: 0,
    };
    pow::mine(&mut header);
    chain.add_block(Block { header, transactions: vec![coinbase] }).unwrap();

    // Chain transfers: wallet[0] -> wallet[1] -> wallet[2] -> wallet[3] -> wallet[4]
    let mut current_amount = 5000u64;
    let fee = 50u64;

    for i in 0..4 {
        let send_amount = current_amount - fee;
        let utxos = chain.get_utxos_for(&wallets[i].pubkey_hash());
        let tx = builder::build_transaction(
            &wallets[i],
            &utxos,
            &wallets[i + 1].address(),
            send_amount,
            fee,
        ).unwrap();

        let height = chain.height + 1;
        let reward = pow::block_reward(height);
        let coinbase = Transaction::new_coinbase(reward, [0u8; 20], height); // burn reward
        let all_txs = vec![coinbase, tx];
        let txids: Vec<_> = all_txs.iter().map(|t| t.txid()).collect();
        let merkle = hash::merkle_root(&txids);

        let mut header = BlockHeader {
            version: 1,
            prev_hash: chain.tip,
            merkle_root: merkle,
            timestamp: 1_700_000_000 + height * 60,
            difficulty: chain.current_difficulty(),
            nonce: 0,
        };
        pow::mine(&mut header);
        chain.add_block(Block { header, transactions: all_txs }).unwrap();

        current_amount = send_amount;
    }

    // Final wallet should have the last amount
    let final_balance = chain.get_balance(&wallets[4].pubkey_hash());
    assert_eq!(final_balance, 5000 - 4 * fee); // 4800
    assert_eq!(chain.height, 5);

    // All intermediate wallets should have 0
    for i in 0..4 {
        assert_eq!(chain.get_balance(&wallets[i].pubkey_hash()), 0);
    }
}
