use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::protocol::{Message, MAX_MESSAGE_SIZE};

#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("message too large: {0} bytes")]
    MessageTooLarge(u32),
    #[error("deserialization error: {0}")]
    Deserialize(#[from] bincode::Error),
    #[error("connection closed")]
    ConnectionClosed,
}

pub struct PeerConnection {
    pub addr: SocketAddr,
    stream: TcpStream,
}

impl PeerConnection {
    pub fn new(addr: SocketAddr, stream: TcpStream) -> Self {
        Self { addr, stream }
    }

    pub async fn send(&mut self, msg: &Message) -> Result<(), PeerError> {
        let bytes = msg.to_bytes();
        self.stream.write_all(&bytes).await?;
        self.stream.flush().await?;
        Ok(())
    }

    pub async fn receive(&mut self) -> Result<Message, PeerError> {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await
            .map_err(|_| PeerError::ConnectionClosed)?;
        let len = u32::from_be_bytes(len_buf);

        if len > MAX_MESSAGE_SIZE {
            return Err(PeerError::MessageTooLarge(len));
        }

        let mut payload = vec![0u8; len as usize];
        self.stream.read_exact(&mut payload).await
            .map_err(|_| PeerError::ConnectionClosed)?;

        Ok(Message::from_bytes(&payload)?)
    }

    pub fn split(self) -> (SocketAddr, TcpStream) {
        (self.addr, self.stream)
    }
}
