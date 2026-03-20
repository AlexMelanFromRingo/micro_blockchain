use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use crate::types::block::{Block, BlockHash};
use crate::types::transaction::Transaction;

/// Network message types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Version {
        height: u32,
        best_hash: BlockHash,
        listen_port: u16,
    },
    VerAck,
    GetBlocks {
        from_hash: BlockHash,
        count: u16,
    },
    Blocks(Vec<Block>),
    NewTx(Transaction),
    GetPeers,
    Peers(Vec<SocketAddr>),
    Ping(u64),
    Pong(u64),
}

impl Message {
    /// Serialize message to length-prefixed binary.
    pub fn to_bytes(&self) -> Vec<u8> {
        let payload = bincode::serialize(self).expect("message serialization");
        let len = payload.len() as u32;
        let mut buf = Vec::with_capacity(4 + payload.len());
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(&payload);
        buf
    }

    /// Deserialize message from payload bytes (without length prefix).
    pub fn from_bytes(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

/// Maximum message size: 4 MB.
pub const MAX_MESSAGE_SIZE: u32 = 4 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_roundtrip() {
        let msg = Message::Ping(42);
        let bytes = msg.to_bytes();
        // Read length
        let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let payload = &bytes[4..];
        assert_eq!(payload.len(), len as usize);
        let decoded = Message::from_bytes(payload).unwrap();
        match decoded {
            Message::Ping(v) => assert_eq!(v, 42),
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_version_message() {
        let msg = Message::Version {
            height: 100,
            best_hash: [0xABu8; 32],
            listen_port: 8333,
        };
        let bytes = msg.to_bytes();
        let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let decoded = Message::from_bytes(&bytes[4..]).unwrap();
        match decoded {
            Message::Version { height, best_hash, listen_port } => {
                assert_eq!(height, 100);
                assert_eq!(listen_port, 8333);
            }
            _ => panic!("wrong message type"),
        }
    }
}
