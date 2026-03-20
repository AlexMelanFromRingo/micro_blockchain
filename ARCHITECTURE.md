# Architecture Reference

Complete module-by-module breakdown of MicroChain. Every public type and function is documented.

---

## Table of Contents

- [crypto](#crypto) - Cryptographic primitives
- [types](#types) - Core data structures
- [consensus](#consensus) - Proof-of-Work and validation
- [chain](#chain) - Blockchain state machine
- [storage](#storage) - Persistent database
- [network](#network) - P2P networking
- [wallet](#wallet) - Key management and transaction building
- [tui](#tui) - Terminal user interface
- [cli](#cli) - Command-line interface

---

## crypto

All cryptographic operations use BLAKE3 for hashing. Signatures use secp256k1 ECDSA via the `k256` crate (pure Rust, `no_std`-compatible).

### crypto/hash.rs

BLAKE3 hashing wrappers.

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `Hash` | `type Hash = [u8; 32]` | 32-byte hash output |
| `hash_bytes` | `fn(data: &[u8]) -> Hash` | Hash arbitrary data with BLAKE3 |
| `hash_pair` | `fn(a: &Hash, b: &Hash) -> Hash` | Hash two concatenated hashes (for Merkle tree internal nodes) |
| `merkle_root` | `fn(hashes: &[Hash]) -> Hash` | Binary Merkle tree root. Odd elements are paired with themselves. Empty input returns `[0; 32]` |
| `derive_key` | `fn(context: &str, material: &[u8]) -> Hash` | BLAKE3 key derivation mode (domain-separated KDF) |

### crypto/keys.rs

Secp256k1 key management.

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `CompressedPubkey` | `type = [u8; 33]` | SEC1 compressed public key |
| `SerializedSignature` | `type = [u8; 64]` | Compact ECDSA signature (r \|\| s) |
| `Keypair` | `struct` | Wraps `k256::ecdsa::SigningKey` |
| `Keypair::generate` | `fn() -> Self` | Generate random keypair using OS RNG |
| `Keypair::from_bytes` | `fn(secret: &[u8; 32]) -> Result<Self>` | Reconstruct from 32-byte secret |
| `Keypair::secret_bytes` | `fn(&self) -> [u8; 32]` | Export raw secret key |
| `Keypair::public_key` | `fn(&self) -> CompressedPubkey` | Get 33-byte compressed public key |
| `Keypair::sign` | `fn(&self, msg_hash: &Hash) -> SerializedSignature` | Sign a 32-byte hash (deterministic RFC 6979) |
| `verify` | `fn(pubkey: &CompressedPubkey, msg_hash: &Hash, sig: &SerializedSignature) -> bool` | Verify ECDSA signature |
| `pubkey_hash` | `fn(pubkey: &CompressedPubkey) -> [u8; 20]` | BLAKE3 hash of pubkey, truncated to 20 bytes |

### crypto/address.rs

Bech32m address encoding. Human-readable part (HRP) is `"mc"` (micro-chain).

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `PubkeyHash` | `type = [u8; 20]` | Address payload |
| `AddressError` | `enum` | `InvalidBech32`, `WrongHrp`, `InvalidLength` |
| `encode_address` | `fn(pubkey: &CompressedPubkey) -> String` | pubkey -> BLAKE3 hash -> Bech32m |
| `encode_address_from_hash` | `fn(hash: &PubkeyHash) -> String` | 20-byte hash -> Bech32m |
| `decode_address` | `fn(address: &str) -> Result<PubkeyHash>` | Bech32m string -> 20-byte hash |

### crypto/mnemonic.rs

BIP39-compatible wordlist with BLAKE3-based checksum and key derivation. **Intentionally non-standard** (not interoperable with Bitcoin wallets).

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `MnemonicError` | `enum` | `InvalidEntropyLength`, `InvalidWordCount`, `InvalidWord`, `InvalidChecksum`, `InvalidWordlist` |
| `entropy_to_mnemonic` | `fn(entropy: &[u8]) -> Result<String>` | 16 bytes -> 12 words, 32 bytes -> 24 words. Checksum = first N bits of BLAKE3(entropy) |
| `generate_mnemonic_12` | `fn() -> Result<String>` | Generate random 12-word mnemonic (128-bit entropy) |
| `generate_mnemonic_24` | `fn() -> Result<String>` | Generate random 24-word mnemonic (256-bit entropy) |
| `mnemonic_to_seed` | `fn(mnemonic: &str, passphrase: &str) -> [u8; 32]` | Derive 32-byte seed via `blake3::derive_key("micro_blockchain mnemonic v1", mnemonic + passphrase)` |
| `validate_mnemonic` | `fn(mnemonic: &str) -> Result<()>` | Verify word count, wordlist membership, and BLAKE3 checksum |

---

## types

Core data structures serialized with `serde` + `bincode`. Large arrays (`[u8; 33]`, `[u8; 64]`) use `serde-big-array`.

### types/block.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `BlockHash` | `type = Hash` | Alias for `[u8; 32]` |
| `BlockHeader` | `struct` | `version: u8`, `prev_hash: BlockHash`, `merkle_root: Hash`, `timestamp: u32`, `difficulty: u32`, `nonce: u32` |
| `BlockHeader::hash` | `fn(&self) -> BlockHash` | Bincode serialize, then BLAKE3 hash |
| `Block` | `struct` | `header: BlockHeader`, `transactions: Vec<Transaction>` |
| `Block::hash` | `fn(&self) -> BlockHash` | Delegates to `header.hash()` |
| `Block::compute_merkle_root` | `fn(&self) -> Hash` | Merkle root from `txid()` of all transactions |
| `Block::verify_merkle_root` | `fn(&self) -> bool` | Check `header.merkle_root == compute_merkle_root()` |

`BlockHeader` fields:
- `version` (u8) - protocol version, currently `1`
- `prev_hash` - hash of previous block header (`[0; 32]` for genesis)
- `merkle_root` - Merkle root of transaction IDs
- `timestamp` (u32) - Unix timestamp in seconds
- `difficulty` (u32) - compact target in nBits format (see [consensus/pow](#consensuspowrs))
- `nonce` (u32) - mined value

### types/transaction.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `TxId` | `type = Hash` | Transaction identifier |
| `PubkeyHash` | `type = [u8; 20]` | Output lock script (pay-to-pubkey-hash) |
| `OutPoint` | `type = (TxId, u16)` | Reference to a specific output (txid + index) |
| `TxInput` | `struct` | `prev_tx_hash: TxId`, `output_index: u16`, `signature: [u8; 64]`, `pubkey: [u8; 33]` |
| `TxOutput` | `struct` | `amount: u64`, `pubkey_hash: PubkeyHash` |
| `Transaction` | `struct` | `inputs: Vec<TxInput>`, `outputs: Vec<TxOutput>` |
| `Transaction::txid` | `fn(&self) -> TxId` | BLAKE3 hash of bincode-serialized transaction |
| `Transaction::is_coinbase` | `fn(&self) -> bool` | `true` if single input with `prev_tx_hash = [0; 32]` and `output_index = 0xFFFF` |
| `Transaction::new_coinbase` | `fn(reward: u64, recipient: PubkeyHash, height: u32) -> Self` | Create coinbase. Height is encoded in the signature field to ensure unique txids across blocks |
| `Transaction::sighash` | `fn(&self, input_index: usize) -> Hash` | Clone tx, zero all signatures, hash. This is what gets signed |
| `Transaction::total_output` | `fn(&self) -> u64` | Sum of all output amounts |

**Coinbase uniqueness:** The `height` parameter is written into the first 4 bytes of the coinbase signature field. Without this, identical reward + recipient would produce identical txids, causing UTXO overwrites.

---

## consensus

### consensus/pow.rs

Proof-of-Work with compact target format and LWMA difficulty adjustment.

#### Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `LWMA_WINDOW` | `60` | Number of blocks in LWMA sliding window |
| `TARGET_BLOCK_TIME` | `60` | Target seconds between blocks |
| `INITIAL_DIFFICULTY` | `0x1F00_FFFF` | Starting compact target (hash[0] must be 0x00, ~256 nonces) |
| `MIN_DIFFICULTY` | `0x2000_FFFF` | Easiest allowed target (floor for difficulty drops) |

#### Compact Target Format

Same concept as Bitcoin's `nBits`:

```
compact = (exponent << 24) | mantissa

exponent E = byte 0 (MSB)
mantissa M = bytes 1-3 (24 bits, big-endian)

Target (32-byte big-endian) = M placed starting at byte (32 - E)
```

Example: `0x1F00FFFF` -> E=31, M=0x00FFFF -> target byte[1..4] = `00 FF FF`, rest zeros. Hash must have first byte = 0x00.

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `compact_to_target` | `fn(compact: u32) -> [u8; 32]` | Expand compact to 32-byte big-endian target |
| `target_to_compact` | `fn(target: &[u8; 32]) -> u32` | Compress 32-byte target to compact |
| `check_pow` | `fn(header: &BlockHeader) -> bool` | Hash header with BLAKE3, check `hash <= target` (byte-by-byte big-endian) |
| `mine` | `fn(header: &mut BlockHeader) -> Option<u32>` | Iterate nonce 0..u32::MAX until `check_pow()` succeeds. Returns winning nonce |
| `block_reward` | `fn(height: u32) -> u64` | 5000 base units, halving every 210,000 blocks. Returns 0 after 64 halvings |

#### Difficulty Score System

LWMA operates on a `u64` "difficulty score" rather than raw 256-bit targets:

```
score = (32 - exponent) * 0x01000000 + (0x00FFFFFF - mantissa)
```

Higher score = harder target. This mapping is monotonic and invertible.

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `compact_to_difficulty_score` | `fn(compact: u32) -> u64` | Convert compact target to u64 score |
| `difficulty_score_to_compact` | `fn(score: u64) -> u32` | Convert u64 score back to compact |

#### LWMA Algorithm

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `lwma_next_difficulty` | `fn(timestamps: &[u64], difficulty_scores: &[u64]) -> u32` | Compute next block's compact difficulty |

Algorithm steps:
1. Take the last `min(N-1, 60)` block intervals
2. For each interval `i` (1 = oldest, N = most recent):
   - `solve_time[i] = timestamp[i] - timestamp[i-1]`, clamped to `[-6T, 6T]`
   - `weighted_sum += solve_time[i] * i`
   - `difficulty_sum += score[i]`
3. `weight_sum = N*(N+1)/2`
4. `avg_difficulty = difficulty_sum / N`
5. `next = avg_difficulty * T * weight_sum / weighted_sum`
6. Clamp to `[avg/2, avg*2]` (max 2x change per block)
7. Clamp to `>= MIN_DIFFICULTY`

### consensus/validation.rs

Block and transaction validation against the UTXO set.

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `ValidationError` | `enum` | 12 variants covering all validation failures |
| `validate_block` | `fn(block, utxo_set, height) -> Result<()>` | Full block validation (see checks below) |
| `validate_transaction` | `fn(tx, utxo_set, spent_in_block) -> Result<u64>` | Validate single non-coinbase tx, return fee |

`validate_block` checks (in order):
1. **PoW** - `check_pow(&block.header)`
2. **Merkle root** - `block.verify_merkle_root()`
3. **Non-empty** - at least one transaction
4. **Coinbase position** - first tx must be coinbase, no others
5. **Coinbase reward** - output <= `block_reward(height) + total_fees`
6. **All non-coinbase txs** - validated individually

`validate_transaction` checks per input:
1. **No double-spend** within the block
2. **UTXO exists** in the set
3. **Pubkey hash matches** the UTXO's lock
4. **Signature valid** against sighash
5. **Total input >= total output** (difference = fee)

**ValidationError variants:**
| Variant | Meaning |
|---------|---------|
| `InvalidPow` | Hash does not meet target |
| `InvalidMerkleRoot` | Header merkle_root != computed |
| `NoCoinbase` | No coinbase transaction |
| `InvalidCoinbaseReward { got, max }` | Coinbase output exceeds allowed reward + fees |
| `DuplicateInput(OutPoint)` | Same UTXO spent twice in block |
| `UnknownUtxo(OutPoint)` | Input references non-existent UTXO |
| `InvalidSignature(usize)` | ECDSA signature verification failed on input N |
| `PubkeyHashMismatch(usize)` | Spending pubkey doesn't match UTXO lock |
| `InsufficientFunds { input, output }` | Outputs exceed inputs |
| `EmptyBlock` | Block has zero transactions |
| `FirstTxNotCoinbase` | First transaction is not a coinbase |
| `MultipleCoinbase` | More than one coinbase in block |

---

## chain

### chain/blockchain.rs

Blockchain state machine: maintains the UTXO set, validates and appends blocks, provides balance queries.

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `ChainError` | `enum` | `Storage(StorageError)`, `Validation(ValidationError)`, `BlockExists`, `PrevBlockNotFound` |
| `Blockchain` | `struct` | Core state: `db`, `utxo_set`, `tip`, `height`, `difficulty_scores`, `timestamps` |
| `Blockchain::new` | `fn(path: &Path) -> Result<Self>` | Load existing chain from disk, or create with genesis block |
| `Blockchain::new_temporary` | `fn() -> Result<Self>` | In-memory chain for testing |
| `Blockchain::add_block` | `fn(&mut self, block: Block) -> Result<BlockHash>` | Validate block, apply UTXO changes, persist, update tip |
| `Blockchain::current_difficulty` | `fn(&self) -> u32` | Returns `INITIAL_DIFFICULTY` for height < 2, else LWMA result |
| `Blockchain::get_utxos_for` | `fn(&self, pkh: &[u8; 20]) -> Vec<(OutPoint, TxOutput)>` | All unspent outputs for a pubkey hash |
| `Blockchain::get_balance` | `fn(&self, pkh: &[u8; 20]) -> u64` | Sum of UTXO amounts for a pubkey hash |
| `Blockchain::get_block` | `fn(&self, hash: &BlockHash) -> Result<Block>` | Retrieve block by hash |
| `Blockchain::get_block_at_height` | `fn(&self, height: u32) -> Result<Block>` | Retrieve block by height |
| `create_genesis_block` | `fn() -> Block` | Coinbase: 5000 units to `[0; 20]`, timestamp `1_700_000_000`, `INITIAL_DIFFICULTY` |

`add_block` sequence:
1. Check block hash not already stored
2. Check `prev_hash == self.tip`
3. `validate_block(block, utxo_set, height + 1)`
4. Apply UTXO changes (remove spent, add new outputs)
5. Push timestamp and difficulty score for LWMA
6. Persist block, height-index, tip, and UTXO deltas

**Fields:**
- `db: Database` - persistent storage handle
- `utxo_set: HashMap<OutPoint, TxOutput>` - in-memory UTXO index
- `tip: BlockHash` - hash of the latest block
- `height: u32` - current chain height
- `timestamps: Vec<u64>` - block timestamps for LWMA
- `difficulty_scores: Vec<u64>` - difficulty scores for LWMA

### chain/mempool.rs

Transaction pool for unconfirmed transactions.

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `MempoolError` | `enum` | `CoinbaseNotAllowed`, `AlreadyExists`, `DoubleSpend`, `Validation(ValidationError)` |
| `Mempool` | `struct` | `HashMap<TxId, Transaction>` |
| `Mempool::new` | `fn() -> Self` | Empty mempool |
| `Mempool::add` | `fn(&mut self, tx, utxo_set) -> Result<TxId>` | Validate and add. Rejects coinbase, duplicates, double-spends |
| `Mempool::remove` | `fn(&mut self, txid: &TxId)` | Remove single tx |
| `Mempool::remove_confirmed` | `fn(&mut self, txids: &[TxId])` | Remove batch of confirmed txs |
| `Mempool::get_mineable` | `fn(&self) -> Vec<Transaction>` | Clone all txs for block template |
| `Mempool::len` | `fn(&self) -> usize` | Count |
| `Mempool::is_empty` | `fn(&self) -> bool` | Check empty |
| `Mempool::contains` | `fn(&self, txid: &TxId) -> bool` | Membership check |
| `Mempool::get` | `fn(&self, txid: &TxId) -> Option<&Transaction>` | Lookup by ID |

---

## storage

### storage/db.rs

Sled-based persistent storage with three trees: `blocks`, `utxos`, `meta`.

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `StorageError` | `enum` | `Sled(sled::Error)`, `Bincode(bincode::Error)`, `NotFound` |
| `Database` | `struct` | `db: sled::Db`, `blocks: sled::Tree`, `utxos: sled::Tree`, `meta: sled::Tree` |
| `Database::open` | `fn(path: &Path) -> Result<Self>` | Open or create persistent database |
| `Database::open_temporary` | `fn() -> Result<Self>` | Open in-memory database (for testing) |

**Block operations:**

| Method | Signature | Description |
|--------|-----------|-------------|
| `put_block` | `fn(&self, hash, block) -> Result<()>` | Store block keyed by hash |
| `get_block` | `fn(&self, hash) -> Result<Block>` | Retrieve block by hash |
| `has_block` | `fn(&self, hash) -> bool` | Check existence |

**UTXO operations:**

| Method | Signature | Description |
|--------|-----------|-------------|
| `put_utxo` | `fn(&self, outpoint, output) -> Result<()>` | Store UTXO |
| `get_utxo` | `fn(&self, outpoint) -> Result<TxOutput>` | Retrieve UTXO |
| `remove_utxo` | `fn(&self, outpoint) -> Result<()>` | Delete spent UTXO |
| `load_utxo_set` | `fn(&self) -> Result<HashMap<OutPoint, TxOutput>>` | Load entire UTXO set into memory |

**Metadata operations:**

| Method | Signature | Description |
|--------|-----------|-------------|
| `set_tip` / `get_tip` | `fn(&self, hash) / fn(&self) -> Result<BlockHash>` | Chain tip hash |
| `set_height` / `get_height` | `fn(&self, u32) / fn(&self) -> Result<u32>` | Chain height |
| `set_block_at_height` / `get_block_at_height` | `fn(&self, u32, hash) / fn(&self, u32) -> Result<BlockHash>` | Height-to-hash index |
| `flush` | `fn(&self) -> Result<()>` | Flush to disk |

---

## network

TCP-based P2P networking with binary message protocol.

### network/protocol.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `MAX_MESSAGE_SIZE` | `const u32 = 4 * 1024 * 1024` | 4 MB max message payload |
| `Message` | `enum` | 9 variants (see below) |
| `Message::to_bytes` | `fn(&self) -> Vec<u8>` | Serialize to `[4-byte BE length][bincode payload]` |
| `Message::from_bytes` | `fn(data: &[u8]) -> Result<Self>` | Deserialize from bincode payload (without length prefix) |

**Message variants:**

| Variant | Fields | Purpose |
|---------|--------|---------|
| `Version` | `height: u32, best_hash: BlockHash, listen_port: u16` | Handshake: announce chain state |
| `VerAck` | - | Handshake acknowledgment |
| `GetBlocks` | `from_hash: BlockHash, count: u16` | Request blocks starting after hash |
| `Blocks` | `Vec<Block>` | Response with block data |
| `NewTx` | `Transaction` | Relay unconfirmed transaction |
| `GetPeers` | - | Request peer addresses |
| `Peers` | `Vec<SocketAddr>` | Response with peer addresses |
| `Ping` | `u64` | Liveness check |
| `Pong` | `u64` | Ping response |

### network/peer.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `PeerError` | `enum` | `Io`, `MessageTooLarge`, `Deserialize`, `ConnectionClosed` |
| `PeerConnection` | `struct` | `addr: SocketAddr`, TCP reader/writer halves |
| `PeerConnection::new` | `fn(addr, TcpStream) -> Self` | Wrap a TCP stream |
| `PeerConnection::send` | `async fn(&mut self, msg: &Message) -> Result<()>` | Write length-prefixed message |
| `PeerConnection::receive` | `async fn(&mut self) -> Result<Message>` | Read and deserialize message |
| `PeerConnection::split` | `fn(self) -> (SocketAddr, TcpStream)` | Deconstruct for advanced use |

### network/server.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `Server` | `struct` | `listen_addr: SocketAddr`, `sync_manager: Arc<SyncManager>` |
| `Server::new` | `fn(addr, sync) -> Self` | Create server |
| `Server::run` | `async fn(&self) -> Result<()>` | Accept TCP connections, spawn `handle_peer` for each |

### network/sync_manager.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `NodeEvent` | `enum` | Events pushed to TUI (see below) |
| `SyncManager` | `struct` | Shared chain/mempool/peer state |
| `SyncManager::new` | `fn(chain, mempool, port, event_tx) -> Self` | Initialize |
| `SyncManager::connect_to_peer` | `async fn(&self, addr) -> Result<()>` | Outbound connection: handshake + sync |
| `SyncManager::handle_peer` | `async fn(&self, peer) -> Result<()>` | Inbound peer: handshake + message loop |
| `SyncManager::peer_count` | `async fn(&self) -> usize` | Number of connected peers |
| `SyncManager::broadcast_block` | `async fn(&self, block: &Block)` | Broadcast block to all peers |
| `SyncManager::broadcast_tx` | `async fn(&self, tx: &Transaction)` | Broadcast transaction to all peers |

**NodeEvent variants:**

| Variant | Fields | Description |
|---------|--------|-------------|
| `NewBlock` | `height: u32, hash: BlockHash` | Block added to chain |
| `NewTx` | `txid: TxId` | Transaction added to mempool |
| `PeerConnected` | `SocketAddr` | New peer connected |
| `PeerDisconnected` | `SocketAddr` | Peer disconnected |
| `SyncProgress` | `height: u32, peer_height: u32` | Sync progress update |

---

## wallet

### wallet/wallet.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `WalletError` | `enum` | `Mnemonic(MnemonicError)`, `Key(k256::ecdsa::Error)` |
| `Wallet` | `struct` | Holds `Keypair`, optional mnemonic string |
| `Wallet::create` | `fn() -> Result<Self>` | Generate 12-word mnemonic, derive keypair |
| `Wallet::from_mnemonic` | `fn(phrase, passphrase) -> Result<Self>` | Validate mnemonic, derive seed, create keypair |
| `Wallet::from_secret` | `fn(secret: &[u8; 32]) -> Result<Self>` | Direct from secret key (no mnemonic) |
| `Wallet::public_key` | `fn(&self) -> [u8; 33]` | Compressed public key |
| `Wallet::pubkey_hash` | `fn(&self) -> [u8; 20]` | BLAKE3(pubkey)[..20] |
| `Wallet::address` | `fn(&self) -> String` | Bech32m address |
| `Wallet::mnemonic` | `fn(&self) -> Option<&str>` | Mnemonic phrase if created with one |
| `Wallet::sign` | `fn(&self, msg: &[u8; 32]) -> [u8; 64]` | Sign hash |
| `Wallet::secret_bytes` | `fn(&self) -> [u8; 32]` | Export secret key |

### wallet/builder.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `BuilderError` | `enum` | `InsufficientFunds { need, have }`, `InvalidAddress(AddressError)`, `NoUtxos` |
| `build_transaction` | `fn(wallet, utxos, to_address, amount, fee) -> Result<Transaction>` | Build, sign, and return a transaction |

`build_transaction` algorithm:
1. Decode destination address (Bech32m -> pubkey hash)
2. Sort UTXOs by amount descending (largest-first selection)
3. Select UTXOs until `accumulated >= amount + fee`
4. Create output to destination
5. If change > 0, create change output back to wallet
6. Sign each input with `wallet.sign(sighash)`
7. Return signed transaction

---

## tui

Terminal UI built with `ratatui` and `crossterm`. Styled after the Blockstream Green wallet.

### Color Palette

| Constant | Value | Usage |
|----------|-------|-------|
| `CYAN` | `#00C3FF` | Accents, active elements, branding |
| `BG` | `#111316` | Main background |
| `BG_PANEL` | `#181A1E` | Panel/card backgrounds |
| `TEXT` | `#C8CDD7` | Normal text |
| `DIM` | `#646973` | Labels, dimmed text |
| `GREEN` | `#00C864` | Positive values, active mining |
| `RED` | `#DC3C3C` | Errors, negative values, stopped |
| `WHITE` | `#F0F2F5` | Emphasized values |

### Layout

```
+--[ MICROCHAIN          0.000 MCH ]--+
| > Home       |  Balance Card        |
|   Txs        |  Chain Info          |
|   Network    |  Activity Log        |
|   Mining     |                      |
+-[ q Quit  m Mine  Tab Next ]--------+
```

### tui/app.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `Tab` | `enum` | `Home`, `Transactions`, `Network`, `Mining` |
| `Tab::ALL` | `const [Tab; 4]` | All tabs in display order |
| `Tab::label` | `fn(&self) -> &'static str` | Display name |
| `Tab::icon` | `fn(&self) -> &'static str` | Placeholder for icon/emoji |
| `TxDirection` | `enum` | `Incoming`, `Outgoing`, `Coinbase` |
| `TxRecord` | `struct` | Display-friendly tx: `txid_short`, `direction`, `amount`, `height`, `timestamp` |
| `App` | `struct` | Full application state (see fields below) |
| `App::new` | `fn(chain, mempool, sync, wallet, event_rx) -> Self` | Initialize with defaults |
| `App::refresh` | `async fn(&mut self)` | Poll chain/mempool/peers/events, update cached display values |
| `App::next_tab` | `fn(&mut self)` | Cycle forward through tabs |
| `App::prev_tab` | `fn(&mut self)` | Cycle backward through tabs |
| `App::format_balance` | `fn(&self) -> String` | Format as `"X.XXX MCH"` (1 MCH = 1000 base units) |

**App cached fields** (updated by `refresh()`):
- `height`, `tip_hash`, `difficulty` - from chain
- `peer_count` - from sync manager
- `mempool_count` - from mempool
- `balance`, `address`, `utxo_count` - from wallet + chain
- `tx_history` - transaction records
- `logs` - activity log (capped at 200 entries)

### tui/event.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `AppEvent` | `enum` | `Key(KeyEvent)`, `Tick` |
| `poll_event` | `fn(timeout: Duration) -> Option<AppEvent>` | Poll for input with timeout |
| `should_quit` | `fn(key) -> bool` | `q` or `Ctrl+C` |
| `toggle_mining` | `fn(key) -> bool` | `m` |
| `next_tab` | `fn(key) -> bool` | `Tab` (without Shift) |
| `prev_tab` | `fn(key) -> bool` | `Shift+Tab` or `BackTab` |

### tui/ui.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `draw` | `fn(f: &mut Frame, app: &App)` | Main render entry point |
| `draw_header` | `fn(f, area, app)` | "MICROCHAIN" branding + balance |
| `draw_sidebar` | `fn(f, area, app)` | Tab list with `>` active indicator |
| `draw_content` | `fn(f, area, app)` | Dispatches to active tab's draw function |
| `draw_home` | `fn(f, area, app)` | Balance card + chain info + activity log |
| `draw_transactions` | `fn(f, area, app)` | Transaction history list with +/- indicators |
| `draw_network` | `fn(f, area, app)` | Peer count + sync status + network log |
| `draw_mining` | `fn(f, area, app)` | Mining status, difficulty, reward, toggle hint |
| `draw_log` | `fn(f, area, app)` | Scrolling activity log (most recent at bottom) |
| `draw_statusbar` | `fn(f, area, app)` | Keybinding hints bar |

---

## cli

### cli/commands.rs

| Symbol | Signature | Description |
|--------|-----------|-------------|
| `Cli` | `struct` | Top-level CLI with `command: Command` and `--datadir` option |
| `Command` | `enum` | `Node`, `Mine`, `Wallet`, `Send`, `Info` |
| `WalletAction` | `enum` | `Create`, `Show { mnemonic }`, `Balance { address }` |
| `run` | `async fn(cli: Cli) -> Result<()>` | Main command dispatcher |

**Commands:**

| Command | Arguments | Description |
|---------|-----------|-------------|
| `node` | `--port 8333`, `--peers addr,...`, `--wallet "mnemonic"` | Start full node with TUI |
| `mine` | `--address mc1...`, `--port 8333` | Headless miner loop |
| `wallet create` | - | Generate new wallet, print mnemonic |
| `wallet show` | `--mnemonic "words"` | Restore wallet, show address |
| `wallet balance` | `--address mc1...` | Show balance (requires running node) |
| `send` | `--to mc1... --amount N --fee N --wallet "mnemonic"` | Build and sign transaction |
| `info` | - | Print chain height, tip, difficulty, UTXO count |

Internal functions:
- `run_node` - Initialize chain, mempool, sync manager, server; connect to peers; start TUI
- `run_tui` - Terminal setup, main event loop (refresh -> draw -> handle input -> mine if active)
- `run_miner` - Headless mining loop with stdout progress
- `run_wallet` - Create/show/balance wallet operations
- `run_send` - Build transaction from mnemonic and UTXO set
- `run_info` - Print chain statistics

The TUI mining loop uses chunked nonce iteration (100,000 nonces per frame) to keep the interface responsive.
