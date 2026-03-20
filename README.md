# MicroChain

Minimal UTXO-based cryptocurrency and blockchain written in Rust. Designed for compactness and educational clarity, targeting environments from Raspberry Pi to full desktops.

## Key Design Choices

| Area | Choice | Why |
|------|--------|-----|
| Hashing | **BLAKE3** everywhere | Fast on 32-bit ARM, single dependency for all hashing |
| Signatures | **secp256k1** (compressed, 33-byte pubkeys) | Standard curve, pure Rust via `k256` |
| Addresses | **Bech32m** with HRP `mc` | Error-detecting, human-readable |
| Mnemonics | BIP39 wordlist + **BLAKE3** checksum & KDF | Intentionally non-standard to avoid confusion with Bitcoin |
| Difficulty | **LWMA** (Linear Weighted Moving Average) | Per-block adjustment, responds quickly to hashrate changes |
| Storage | **sled** embedded KV | Zero-config, pure Rust |
| Serialization | **bincode** | Compact binary, no schema overhead |
| TUI | **ratatui** + crossterm | Blockstream Green-inspired dark theme with cyan accents |
| Binary size | ~1.9 MB release | `opt-level="z"`, LTO, strip, `panic="abort"` |

## Quick Start

```bash
# Build
cargo build --release

# Create a wallet
./target/release/micro_blockchain wallet create

# Start a node with TUI
./target/release/micro_blockchain node --wallet "your twelve word mnemonic phrase here ..."

# Start a standalone miner
./target/release/micro_blockchain mine --address mc1...

# Show blockchain info
./target/release/micro_blockchain info

# Send a transaction
./target/release/micro_blockchain send \
  --to mc1... \
  --amount 1000 \
  --fee 10 \
  --wallet "your mnemonic phrase"
```

## TUI

The terminal interface is styled after the Blockstream Green wallet:

- **Dark background** (`#111316`) with **cyan `#00C3FF`** accents
- Sidebar navigation: Home, Transactions, Network, Mining
- `[Tab]` / `[Shift+Tab]` to navigate, `[m]` to toggle mining, `[q]` to quit

### Panels

| Tab | Content |
|-----|---------|
| Home | Balance card, chain info (height, tip, difficulty, mempool), activity log |
| Transactions | Transaction history with direction indicators (+/-) |
| Network | Connected peer count, sync status, network log |
| Mining | Status, difficulty, block height, next reward, toggle control |

## Architecture

```
src/
  crypto/       BLAKE3 hashing, secp256k1 keys, Bech32m addresses, BIP39-like mnemonics
  types/        Block, BlockHeader, Transaction, TxInput, TxOutput
  consensus/    PoW mining, compact target format, LWMA difficulty, block/tx validation
  chain/        Blockchain state (UTXO set, tip, height), mempool
  storage/      sled database wrapper (blocks, UTXOs, metadata)
  network/      TCP P2P protocol, peer connections, block/tx sync
  wallet/       Key management, mnemonic wallets, transaction builder
  tui/          Ratatui terminal UI (app state, event handling, rendering)
  cli/          Clap CLI (node, mine, wallet, send, info commands)
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for a detailed module-by-module breakdown with every function documented.

## Consensus

### Proof of Work

Compact target format (same concept as Bitcoin's `nBits`):

```
Byte 0 (MSB): exponent E
Bytes 1-3:    mantissa M (big-endian)
Target = M * 256^(E-3)
```

The miner hashes the block header with BLAKE3 and checks `hash <= target`.

### LWMA Difficulty Adjustment

Instead of Bitcoin's 2016-block interval retarget, MicroChain adjusts difficulty **every block** using Zawy's LWMA algorithm (used by Grin, Masari, Haven):

- **Window:** 60 blocks
- **Target block time:** 60 seconds
- **Weighting:** linear (most recent block gets highest weight)
- **Solve time clamping:** `[-6T, 6T]` to resist timestamp manipulation
- **Change cap:** max 2x per block

```
next_difficulty = avg_difficulty * T * weight_sum / weighted_solvetime_sum
```

### Block Reward

5000 base units (5.000 MCH), halving every 210,000 blocks. 1 MCH = 1000 base units.

## P2P Network

Binary length-prefixed protocol over TCP:

```
[4 bytes: payload length (BE)] [payload: bincode-serialized Message]
```

Message types: `Version`, `VerAck`, `GetBlocks`, `Blocks`, `NewTx`, `GetPeers`, `Peers`, `Ping`, `Pong`.

Nodes perform initial block sync on connection and relay new blocks/transactions.

## Testing

```bash
# Run all tests (68 unit + 13 integration)
cargo test

# Run only unit tests
cargo test --lib

# Run only integration tests
cargo test --test integration
```

### Test Coverage

- **Crypto:** hash determinism, key generation/signing/verification, address roundtrip, mnemonic generation/validation
- **Types:** block hashing, coinbase detection, sighash computation
- **Consensus:** compact target roundtrip, PoW verification, LWMA (stable/fast/slow blocks), reward halving
- **Chain:** genesis creation, block addition, multi-block mining, LWMA adjustment
- **Mempool:** valid tx acceptance, coinbase/duplicate/double-spend rejection
- **Storage:** block/UTXO/metadata persistence roundtrip
- **Wallet:** creation, mnemonic determinism, sign/verify
- **Builder:** transaction building, insufficient funds, signature validation
- **Network:** message serialization, two-node sync, transaction propagation
- **Stress:** 100-block mining, 5-transfer chain

## License

MIT
