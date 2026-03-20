use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;

use super::peer::PeerConnection;
use super::sync_manager::SyncManager;

pub struct Server {
    listen_addr: SocketAddr,
    sync_manager: Arc<SyncManager>,
}

impl Server {
    pub fn new(listen_addr: SocketAddr, sync_manager: Arc<SyncManager>) -> Self {
        Self { listen_addr, sync_manager }
    }

    pub async fn run(&self) -> Result<(), std::io::Error> {
        let listener = TcpListener::bind(self.listen_addr).await?;
        tracing::info!("Listening on {}", self.listen_addr);

        loop {
            let (stream, addr) = listener.accept().await?;
            tracing::info!("New peer connected: {}", addr);

            let sync = self.sync_manager.clone();
            tokio::spawn(async move {
                let mut peer = PeerConnection::new(addr, stream);
                if let Err(e) = sync.handle_peer(&mut peer).await {
                    tracing::warn!("Peer {} error: {}", addr, e);
                }
            });
        }
    }
}
