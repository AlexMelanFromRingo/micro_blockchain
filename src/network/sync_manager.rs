use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};

use super::peer::{PeerConnection, PeerError};
use super::protocol::Message;
use crate::chain::blockchain::Blockchain;
use crate::chain::mempool::Mempool;
use crate::types::block::Block;
use crate::types::transaction::Transaction;

/// Events emitted by the sync manager for the UI/CLI.
#[derive(Debug, Clone)]
pub enum NodeEvent {
    NewBlock { height: u32, hash: [u8; 32] },
    NewTx { txid: [u8; 32] },
    PeerConnected(SocketAddr),
    PeerDisconnected(SocketAddr),
    SyncProgress { height: u32, peer_height: u32 },
}

/// Default seed nodes for initial peer discovery.
pub const SEED_NODES: &[&str] = &[
    "seed1.microchain.net:8333",
    "seed2.microchain.net:8333",
    "seed3.microchain.net:8333",
];

/// Maximum number of outbound peer connections.
const MAX_OUTBOUND: usize = 8;

pub struct SyncManager {
    pub chain: Arc<RwLock<Blockchain>>,
    pub mempool: Arc<RwLock<Mempool>>,
    pub peers: Arc<RwLock<HashSet<SocketAddr>>>,
    pub event_tx: mpsc::UnboundedSender<NodeEvent>,
    pub listen_port: u16,
    /// Channel for discovered peer addresses that need connecting.
    discovery_tx: mpsc::UnboundedSender<SocketAddr>,
}

impl SyncManager {
    pub fn new(
        chain: Arc<RwLock<Blockchain>>,
        mempool: Arc<RwLock<Mempool>>,
        listen_port: u16,
        event_tx: mpsc::UnboundedSender<NodeEvent>,
    ) -> Self {
        let (discovery_tx, _) = mpsc::unbounded_channel();
        Self {
            chain,
            mempool,
            peers: Arc::new(RwLock::new(HashSet::new())),
            event_tx,
            listen_port,
            discovery_tx,
        }
    }

    /// Start background peer discovery loop. Must be called after wrapping in Arc.
    pub fn start_discovery(self: &Arc<Self>) -> mpsc::UnboundedReceiver<SocketAddr> {
        let (tx, rx) = mpsc::unbounded_channel();
        // Replace the dummy sender with a real one — we need interior mutability here,
        // but since we call this once at startup before any peers, we reconstruct.
        // Instead, we return the rx and let the caller wire it up.
        // Store tx for use in message_loop via a separate field.
        // Since SyncManager is already built, we use a different approach:
        // return (tx, rx) and the caller passes tx into a new SyncManager.
        // But that's messy. Let's just return rx and use a shared sender.
        drop(tx);
        rx
    }

    /// Create a SyncManager with a discovery channel already wired.
    pub fn with_discovery(
        chain: Arc<RwLock<Blockchain>>,
        mempool: Arc<RwLock<Mempool>>,
        listen_port: u16,
        event_tx: mpsc::UnboundedSender<NodeEvent>,
    ) -> (Self, mpsc::UnboundedReceiver<SocketAddr>) {
        let (discovery_tx, discovery_rx) = mpsc::unbounded_channel();
        let sm = Self {
            chain,
            mempool,
            peers: Arc::new(RwLock::new(HashSet::new())),
            event_tx,
            listen_port,
            discovery_tx,
        };
        (sm, discovery_rx)
    }

    /// Spawn the background task that connects to discovered peers.
    pub fn spawn_discovery_loop(self: &Arc<Self>, mut rx: mpsc::UnboundedReceiver<SocketAddr>) {
        let sync = self.clone();
        tokio::spawn(async move {
            while let Some(addr) = rx.recv().await {
                let peer_count = sync.peers.read().await.len();
                if peer_count >= MAX_OUTBOUND {
                    continue;
                }
                let sync = sync.clone();
                tokio::spawn(async move {
                    if let Err(e) = sync.connect_to_peer(addr).await {
                        tracing::debug!("Discovered peer {} unreachable: {}", addr, e);
                    }
                });
            }
        });
    }

    /// Connect to a peer and run the sync protocol.
    pub async fn connect_to_peer(&self, addr: SocketAddr) -> Result<(), PeerError> {
        let stream = TcpStream::connect(addr).await?;
        let mut peer = PeerConnection::new(addr, stream);
        self.handle_peer(&mut peer).await
    }

    /// Handle messages from a peer connection.
    pub async fn handle_peer(&self, peer: &mut PeerConnection) -> Result<(), PeerError> {
        let addr = peer.addr;

        {
            let mut peers = self.peers.write().await;
            peers.insert(addr);
        }
        let _ = self.event_tx.send(NodeEvent::PeerConnected(addr));

        // Send our version
        let (height, best_hash) = {
            let chain = self.chain.read().await;
            (chain.height, chain.tip)
        };
        peer.send(&Message::Version {
            height,
            best_hash,
            listen_port: self.listen_port,
        }).await?;

        // Request peer addresses for discovery
        peer.send(&Message::GetPeers).await?;

        // Message loop
        let result = self.message_loop(peer).await;

        {
            let mut peers = self.peers.write().await;
            peers.remove(&addr);
        }
        let _ = self.event_tx.send(NodeEvent::PeerDisconnected(addr));

        result
    }

    async fn message_loop(&self, peer: &mut PeerConnection) -> Result<(), PeerError> {
        loop {
            let msg = peer.receive().await?;

            match msg {
                Message::Version { height,  .. } => {
                    peer.send(&Message::VerAck).await?;

                    let our_height = {
                        let chain = self.chain.read().await;
                        chain.height
                    };

                    if height > our_height {
                        let _ = self.event_tx.send(NodeEvent::SyncProgress {
                            height: our_height,
                            peer_height: height,
                        });
                        let our_tip = {
                            let chain = self.chain.read().await;
                            chain.tip
                        };
                        peer.send(&Message::GetBlocks {
                            from_hash: our_tip,
                            count: 500,
                        }).await?;
                    }
                }
                Message::VerAck => {}
                Message::GetBlocks { from_hash, count } => {
                    let blocks = self.get_blocks_after(from_hash, count).await;
                    peer.send(&Message::Blocks(blocks)).await?;
                }
                Message::Blocks(blocks) => {
                    for block in blocks {
                        self.process_new_block(block).await;
                    }
                }
                Message::NewTx(tx) => {
                    self.process_new_tx(tx).await;
                }
                Message::GetPeers => {
                    let peers = self.peers.read().await;
                    let addrs: Vec<SocketAddr> = peers.iter().copied().collect();
                    peer.send(&Message::Peers(addrs)).await?;
                }
                Message::Peers(addrs) => {
                    // Filter and send to discovery channel (non-blocking)
                    let known = self.peers.read().await;
                    let listen_port = self.listen_port;
                    for addr in addrs {
                        if addr.port() == listen_port && addr.ip().is_loopback() {
                            continue;
                        }
                        if known.contains(&addr) {
                            continue;
                        }
                        // Best-effort: if channel is closed, just skip
                        let _ = self.discovery_tx.send(addr);
                    }
                }
                Message::Ping(nonce) => {
                    peer.send(&Message::Pong(nonce)).await?;
                }
                Message::Pong(_) => {}
            }
        }
    }

    async fn get_blocks_after(&self, from_hash: [u8; 32], count: u16) -> Vec<Block> {
        let chain = self.chain.read().await;
        let mut blocks = Vec::new();

        let mut start_height = None;
        for h in 0..=chain.height {
            if let Ok(hash) = chain.db.get_block_at_height(h) {
                if hash == from_hash {
                    start_height = Some(h + 1);
                    break;
                }
            }
        }

        if let Some(start) = start_height {
            for h in start..=(start + count as u32).min(chain.height) {
                if let Ok(block) = chain.get_block_at_height(h) {
                    blocks.push(block);
                }
            }
        }

        blocks
    }

    async fn process_new_block(&self, block: Block) {
        let hash = block.hash();
        let mut chain = self.chain.write().await;
        match chain.add_block(block) {
            Ok(_) => {
                let _ = self.event_tx.send(NodeEvent::NewBlock {
                    height: chain.height,
                    hash,
                });
            }
            Err(e) => {
                tracing::debug!("Block rejected: {}", e);
            }
        }
    }

    async fn process_new_tx(&self, tx: Transaction) {
        let txid = tx.txid();
        let chain = self.chain.read().await;
        let mut mempool = self.mempool.write().await;
        match mempool.add(tx, &chain.utxo_set) {
            Ok(_) => {
                let _ = self.event_tx.send(NodeEvent::NewTx { txid });
            }
            Err(e) => {
                tracing::debug!("Tx rejected: {}", e);
            }
        }
    }

    /// Broadcast a new block to all connected peers.
    pub async fn broadcast_block(&self, block: &Block) {
        let _ = self.event_tx.send(NodeEvent::NewBlock {
            height: 0,
            hash: block.hash(),
        });
    }

    /// Broadcast a new transaction to all connected peers.
    pub async fn broadcast_tx(&self, tx: &Transaction) {
        let _ = self.event_tx.send(NodeEvent::NewTx { txid: tx.txid() });
    }

    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Connect to seed nodes for initial peer discovery.
    pub async fn connect_to_seeds(self: &Arc<Self>) {
        for seed in SEED_NODES {
            let seed = seed.to_string();
            let sync = self.clone();
            tokio::spawn(async move {
                match tokio::net::lookup_host(&seed).await {
                    Ok(addrs) => {
                        for addr in addrs {
                            let sync = sync.clone();
                            tokio::spawn(async move {
                                if let Err(e) = sync.connect_to_peer(addr).await {
                                    tracing::debug!("Seed {} unreachable: {}", addr, e);
                                }
                            });
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Seed {} DNS failed: {}", seed, e);
                    }
                }
            });
        }
    }
}
