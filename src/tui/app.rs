use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};

use crate::chain::blockchain::Blockchain;
use crate::chain::mempool::Mempool;
use crate::network::sync_manager::{NodeEvent, SyncManager};
use crate::wallet::wallet::Wallet;

pub struct App {
    pub chain: Arc<RwLock<Blockchain>>,
    pub mempool: Arc<RwLock<Mempool>>,
    pub sync_manager: Arc<SyncManager>,
    pub wallet: Option<Wallet>,
    pub event_rx: mpsc::UnboundedReceiver<NodeEvent>,
    pub logs: Vec<String>,
    pub running: bool,
    pub mining: bool,
    pub selected_tab: usize,

    // Cached display values
    pub height: u32,
    pub tip_hash: String,
    pub difficulty: u32,
    pub peer_count: usize,
    pub mempool_count: usize,
    pub balance: u64,
    pub address: String,
}

impl App {
    pub fn new(
        chain: Arc<RwLock<Blockchain>>,
        mempool: Arc<RwLock<Mempool>>,
        sync_manager: Arc<SyncManager>,
        wallet: Option<Wallet>,
        event_rx: mpsc::UnboundedReceiver<NodeEvent>,
    ) -> Self {
        let address = wallet.as_ref().map(|w| w.address()).unwrap_or_default();
        Self {
            chain,
            mempool,
            sync_manager,
            wallet,
            event_rx,
            logs: vec!["Node started.".into()],
            running: true,
            mining: false,
            selected_tab: 0,
            height: 0,
            tip_hash: String::new(),
            difficulty: 0,
            peer_count: 0,
            mempool_count: 0,
            balance: 0,
            address,
        }
    }

    pub async fn refresh(&mut self) {
        let chain = self.chain.read().await;
        self.height = chain.height;
        self.tip_hash = hex::encode(&chain.tip[..8]);
        self.difficulty = chain.current_difficulty();

        if let Some(ref w) = self.wallet {
            self.balance = chain.get_balance(&w.pubkey_hash());
        }
        drop(chain);

        self.peer_count = self.sync_manager.peer_count().await;
        self.mempool_count = self.mempool.read().await.len();

        // Drain events
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                NodeEvent::NewBlock { height, hash } => {
                    self.logs.push(format!("Block #{height} {}", hex::encode(&hash[..8])));
                }
                NodeEvent::NewTx { txid } => {
                    self.logs.push(format!("New tx {}", hex::encode(&txid[..8])));
                }
                NodeEvent::PeerConnected(addr) => {
                    self.logs.push(format!("Peer connected: {addr}"));
                }
                NodeEvent::PeerDisconnected(addr) => {
                    self.logs.push(format!("Peer disconnected: {addr}"));
                }
                NodeEvent::SyncProgress { height, peer_height } => {
                    self.logs.push(format!("Syncing: {height}/{peer_height}"));
                }
            }
        }

        // Keep log size bounded
        if self.logs.len() > 100 {
            self.logs.drain(..self.logs.len() - 100);
        }
    }
}
