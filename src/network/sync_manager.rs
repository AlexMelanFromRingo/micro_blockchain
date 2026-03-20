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

pub struct SyncManager {
    pub chain: Arc<RwLock<Blockchain>>,
    pub mempool: Arc<RwLock<Mempool>>,
    pub peers: Arc<RwLock<HashSet<SocketAddr>>>,
    pub event_tx: mpsc::UnboundedSender<NodeEvent>,
    pub listen_port: u16,
}

impl SyncManager {
    pub fn new(
        chain: Arc<RwLock<Blockchain>>,
        mempool: Arc<RwLock<Mempool>>,
        listen_port: u16,
        event_tx: mpsc::UnboundedSender<NodeEvent>,
    ) -> Self {
        Self {
            chain,
            mempool,
            peers: Arc::new(RwLock::new(HashSet::new())),
            event_tx,
            listen_port,
        }
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
                    // Could connect to new peers here
                    tracing::debug!("Received {} peer addresses", addrs.len());
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

        // Find the height of from_hash
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
        // In a real implementation, we'd keep peer write handles.
        // For now, this is a placeholder for the event system.
        let _ = self.event_tx.send(NodeEvent::NewBlock {
            height: 0, // caller should provide
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
}
