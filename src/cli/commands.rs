use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tokio::sync::{mpsc, RwLock};

use crate::chain::blockchain::Blockchain;
use crate::chain::mempool::Mempool;
use crate::consensus::pow;
use crate::crypto::hash;
use crate::network::server::Server;
use crate::network::sync_manager::SyncManager;
use crate::tui::app::App;
use crate::types::block::{Block, BlockHeader};
use crate::types::transaction::Transaction;
use crate::wallet::builder;
use crate::wallet::wallet::Wallet;

#[derive(Parser)]
#[command(name = "microchain", version = "0.1.0", about = "Minimal UTXO blockchain")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Data directory
    #[arg(long, default_value = "data")]
    pub datadir: PathBuf,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start a full node with TUI
    Node {
        #[arg(long, default_value = "8333")]
        port: u16,
        /// Comma-separated peer addresses
        #[arg(long)]
        peers: Option<String>,
        /// Path to wallet file or mnemonic
        #[arg(long)]
        wallet: Option<String>,
    },
    /// Start mining
    Mine {
        #[arg(long)]
        address: String,
        #[arg(long, default_value = "8333")]
        port: u16,
    },
    /// Wallet operations
    Wallet {
        #[command(subcommand)]
        action: WalletAction,
    },
    /// Send a transaction
    Send {
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u64,
        #[arg(long, default_value = "10")]
        fee: u64,
        #[arg(long)]
        wallet: String,
    },
    /// Show blockchain info
    Info,
}

#[derive(Subcommand)]
pub enum WalletAction {
    /// Create a new wallet
    Create,
    /// Show wallet info from mnemonic
    Show {
        #[arg(long)]
        mnemonic: String,
    },
    /// Show balance
    Balance {
        #[arg(long)]
        address: String,
    },
}

pub async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Node { port, peers, wallet } => {
            run_node(cli.datadir, port, peers, wallet).await
        }
        Command::Mine { address, port } => {
            run_miner(cli.datadir, address, port).await
        }
        Command::Wallet { action } => {
            run_wallet(action)
        }
        Command::Send { to, amount, fee, wallet } => {
            run_send(cli.datadir, to, amount, fee, wallet).await
        }
        Command::Info => {
            run_info(cli.datadir)
        }
    }
}

async fn run_node(
    datadir: PathBuf,
    port: u16,
    peers: Option<String>,
    wallet_mnemonic: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let chain = Blockchain::new(&datadir.join("chain"))?;
    let chain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new()));
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    let listen_addr: SocketAddr = format!("0.0.0.0:{port}").parse()?;
    let sync = Arc::new(SyncManager::new(
        chain.clone(),
        mempool.clone(),
        port,
        event_tx,
    ));

    // Start server
    let server = Server::new(listen_addr, sync.clone());
    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            tracing::error!("Server error: {}", e);
        }
    });

    // Connect to initial peers
    if let Some(peer_list) = peers {
        for addr_str in peer_list.split(',') {
            if let Ok(addr) = addr_str.trim().parse::<SocketAddr>() {
                let sync = sync.clone();
                tokio::spawn(async move {
                    if let Err(e) = sync.connect_to_peer(addr).await {
                        tracing::warn!("Failed to connect to {}: {}", addr, e);
                    }
                });
            }
        }
    }

    // Load wallet if provided
    let wallet = if let Some(mnemonic) = wallet_mnemonic {
        Some(Wallet::from_mnemonic(&mnemonic, "")?)
    } else {
        None
    };

    // Run TUI
    run_tui(chain, mempool, sync, wallet, event_rx).await
}

async fn run_tui(
    chain: Arc<RwLock<Blockchain>>,
    mempool: Arc<RwLock<Mempool>>,
    sync: Arc<SyncManager>,
    wallet: Option<Wallet>,
    event_rx: mpsc::UnboundedReceiver<crate::network::sync_manager::NodeEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::{execute, terminal};
    use ratatui::backend::CrosstermBackend;
    use ratatui::Terminal;

    terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, terminal::EnterAlternateScreen, crossterm::cursor::Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(chain, mempool, sync, wallet, event_rx);

    while app.running {
        app.refresh().await;
        terminal.draw(|f| crate::tui::ui::draw(f, &app))?;

        if let Some(event) = crate::tui::event::poll_event(Duration::from_millis(250)) {
            match event {
                crate::tui::event::AppEvent::Key(key) => {
                    if crate::tui::event::should_quit(&key) {
                        app.running = false;
                    } else if crate::tui::event::toggle_mining(&key) {
                        app.mining = !app.mining;
                        let state = if app.mining { "ON" } else { "OFF" };
                        app.logs.push(format!("Mining: {state}"));
                    } else if crate::tui::event::next_tab(&key) {
                        app.selected_tab = (app.selected_tab + 1) % 4;
                    }
                }
                crate::tui::event::AppEvent::Tick => {}
            }
        }

        // Simple mining loop (mine one block per iteration if enabled)
        if app.mining {
            if let Some(ref w) = app.wallet {
                let mut chain = app.chain.write().await;
                let difficulty = chain.current_difficulty();
                let height = chain.height + 1;
                let reward = pow::block_reward(height);
                let coinbase = Transaction::new_coinbase(reward, w.pubkey_hash(), height);

                // Include mempool txs
                let mempool_txs = app.mempool.read().await.get_mineable();
                let mut all_txs = vec![coinbase];
                all_txs.extend(mempool_txs);

                let txids: Vec<_> = all_txs.iter().map(|tx| tx.txid()).collect();
                let merkle = hash::merkle_root(&txids);

                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as u32;

                let mut header = BlockHeader {
                    version: 1,
                    prev_hash: chain.tip,
                    merkle_root: merkle,
                    timestamp,
                    difficulty,
                    nonce: 0,
                };

                // Mine with limited iterations to keep TUI responsive
                let mut found = false;
                for nonce in 0..100_000u32 {
                    header.nonce = nonce;
                    if pow::check_pow(&header) {
                        found = true;
                        break;
                    }
                }

                if found {
                    let block = Block { header, transactions: all_txs };
                    match chain.add_block(block) {
                        Ok(hash) => {
                            app.logs.push(format!("Mined block #{} {}", chain.height, hex::encode(&hash[..8])));
                            // Remove confirmed txs from mempool
                            let confirmed: Vec<_> = txids[1..].to_vec();
                            app.mempool.write().await.remove_confirmed(&confirmed);
                        }
                        Err(e) => {
                            app.logs.push(format!("Mining error: {e}"));
                        }
                    }
                }
            }
        }
    }

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        terminal::LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;

    Ok(())
}

async fn run_miner(
    datadir: PathBuf,
    address: String,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let pkh = crate::crypto::address::decode_address(&address)?;
    let mut chain = Blockchain::new(&datadir.join("chain"))?;

    println!("Mining to address: {address}");
    println!("Starting at height: {}", chain.height);

    loop {
        let difficulty = chain.current_difficulty();
        let height = chain.height + 1;
        let reward = pow::block_reward(height);
        let coinbase = Transaction::new_coinbase(reward, pkh, height);
        let merkle = hash::merkle_root(&[coinbase.txid()]);

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32;

        let mut header = BlockHeader {
            version: 1,
            prev_hash: chain.tip,
            merkle_root: merkle,
            timestamp,
            difficulty,
            nonce: 0,
        };

        print!("Mining block #{height} (diff={difficulty})...");
        if let Some(nonce) = pow::mine(&mut header) {
            let block = Block { header, transactions: vec![coinbase] };
            let hash = chain.add_block(block)?;
            println!(" found! nonce={nonce} hash={}", hex::encode(&hash[..8]));
        } else {
            println!(" nonce space exhausted, retrying with new timestamp");
        }
    }
}

fn run_wallet(action: WalletAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        WalletAction::Create => {
            let w = Wallet::create()?;
            println!("Wallet created!");
            println!("Mnemonic: {}", w.mnemonic().unwrap());
            println!("Address:  {}", w.address());
            println!("Pubkey:   {}", hex::encode(w.public_key()));
            println!("\nSave your mnemonic phrase! It cannot be recovered.");
        }
        WalletAction::Show { mnemonic } => {
            let w = Wallet::from_mnemonic(&mnemonic, "")?;
            println!("Address: {}", w.address());
            println!("Pubkey:  {}", hex::encode(w.public_key()));
        }
        WalletAction::Balance { address } => {
            println!("Balance check requires a running node.");
            println!("Use: microchain node --wallet <mnemonic>");
        }
    }
    Ok(())
}

async fn run_send(
    datadir: PathBuf,
    to: String,
    amount: u64,
    fee: u64,
    mnemonic: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let wallet = Wallet::from_mnemonic(&mnemonic, "")?;
    let chain = Blockchain::new(&datadir.join("chain"))?;

    let utxos = chain.get_utxos_for(&wallet.pubkey_hash());
    let tx = builder::build_transaction(&wallet, &utxos, &to, amount, fee)?;

    println!("Transaction built!");
    println!("TxID:    {}", hex::encode(tx.txid()));
    println!("Inputs:  {}", tx.inputs.len());
    println!("Outputs: {}", tx.outputs.len());
    println!("Total:   {} + {} fee", amount, fee);

    // In a full implementation, this would broadcast to the network
    println!("\nNote: Transaction created but broadcast requires a running node.");

    Ok(())
}

fn run_info(datadir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let chain = Blockchain::new(&datadir.join("chain"))?;
    println!("Blockchain Info:");
    println!("  Height:     {}", chain.height);
    println!("  Tip:        {}", hex::encode(chain.tip));
    println!("  Difficulty:  {}", chain.current_difficulty());
    println!("  UTXO count: {}", chain.utxo_set.len());
    Ok(())
}
