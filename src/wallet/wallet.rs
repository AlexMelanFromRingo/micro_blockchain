use crate::crypto::address;
use crate::crypto::keys::{self, Keypair};
use crate::crypto::mnemonic;

pub struct Wallet {
    keypair: Keypair,
    mnemonic_phrase: Option<String>,
}

impl Wallet {
    /// Create a new wallet with a fresh 12-word mnemonic.
    pub fn create() -> Result<Self, WalletError> {
        let phrase = mnemonic::generate_mnemonic_12()
            .map_err(|e| WalletError::Mnemonic(e.to_string()))?;
        Self::from_mnemonic(&phrase, "")
    }

    /// Restore a wallet from a mnemonic phrase.
    pub fn from_mnemonic(phrase: &str, passphrase: &str) -> Result<Self, WalletError> {
        mnemonic::validate_mnemonic(phrase)
            .map_err(|e| WalletError::Mnemonic(e.to_string()))?;

        let seed = mnemonic::mnemonic_to_seed(phrase, passphrase);
        let keypair = Keypair::from_bytes(&seed)
            .map_err(|e| WalletError::Key(e.to_string()))?;

        Ok(Self {
            keypair,
            mnemonic_phrase: Some(phrase.to_string()),
        })
    }

    /// Create a wallet from a raw secret key.
    pub fn from_secret(secret: &[u8; 32]) -> Result<Self, WalletError> {
        let keypair = Keypair::from_bytes(secret)
            .map_err(|e| WalletError::Key(e.to_string()))?;
        Ok(Self { keypair, mnemonic_phrase: None })
    }

    pub fn public_key(&self) -> [u8; 33] {
        self.keypair.public_key()
    }

    pub fn pubkey_hash(&self) -> [u8; 20] {
        keys::pubkey_hash(&self.keypair.public_key())
    }

    pub fn address(&self) -> String {
        address::encode_address(&self.keypair.public_key())
    }

    pub fn mnemonic(&self) -> Option<&str> {
        self.mnemonic_phrase.as_deref()
    }

    pub fn sign(&self, msg: &[u8; 32]) -> [u8; 64] {
        self.keypair.sign(msg)
    }

    pub fn secret_bytes(&self) -> [u8; 32] {
        self.keypair.secret_bytes()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("mnemonic error: {0}")]
    Mnemonic(String),
    #[error("key error: {0}")]
    Key(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_wallet() {
        let w = Wallet::create().unwrap();
        assert!(w.mnemonic().is_some());
        assert!(w.address().starts_with("mc1"));
    }

    #[test]
    fn test_wallet_from_mnemonic_deterministic() {
        let w1 = Wallet::create().unwrap();
        let phrase = w1.mnemonic().unwrap().to_string();
        let w2 = Wallet::from_mnemonic(&phrase, "").unwrap();
        assert_eq!(w1.address(), w2.address());
        assert_eq!(w1.public_key(), w2.public_key());
    }

    #[test]
    fn test_wallet_sign_verify() {
        let w = Wallet::create().unwrap();
        let msg = crate::crypto::hash::hash_bytes(b"test");
        let sig = w.sign(&msg);
        assert!(keys::verify(&w.public_key(), &msg, &sig));
    }
}
