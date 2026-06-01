# TinyVPN 🌐

A minimal open-source mesh VPN built in Rust.

Inspired by [Tailscale](https://tailscale.com) / [CloudNet](https://cloudnet.world) / [ZeroTier](https://www.zerotier.com).

## Architecture

```
┌─────────────┐       ┌─────────────┐
│   Node A    │◄─────►│   Node B    │
│  (Client)   │  P2P  │  (Client)   │
└──────┬──────┘       └──────┬──────┘
       │                     │
       │   Control Plane     │
       └──────►┌─────┐◄─────┘
               │ CCS │  (Coordinator)
               └─────┘
```

### Components

| Crate | Description |
|-------|-------------|
| `tinyvpn-ccs` | Control plane server — node registration, key exchange, topology, STUN |
| `tinyvpn-cli` | CLI client — register, connect, status, disconnect |
| `tinyvpn-core` | Shared library — crypto, protocol, WireGuard interface management |
| `tinyvpn-p2p` | Peer-to-peer engine — NAT traversal, hole punching |
| `tinyvpn-relay` | Relay server — UDP forwarding when hole punching fails |

### MVP Scope

- [x] Project structure
- [x] Two nodes communicate via WireGuard tunnel
- [x] Control server coordinates peer discovery & key exchange
- [x] Basic NAT hole punching (UDP)
- [x] Relay fallback when P2P fails
- [x] CLI: `register`, `connect`, `status`, `disconnect`

### Quick Start

```bash
# Build everything
cargo build

# Start control server
cargo run -p tinyvpn-ccs

# Start relay server
cargo run -p tinyvpn-relay

# On Node A
cargo run -p tinyvpn-cli -- register --name node-a
cargo run -p tinyvpn-cli -- connect

# On Node B
cargo run -p tinyvpn-cli -- register --name node-b
cargo run -p tinyvpn-cli -- connect

# Now Node A and B can ping each other via WireGuard IPs
```

## Tech Stack

- **Language:** Rust
- **Networking:** Tokio + quinn (QUIC for control plane), UDP for data plane
- **VPN Tunnel:** WireGuard (via `wg-quick` / `wireguard-uapi`)
- **NAT Traversal:** STUN-based hole punching
- **Crypto:** `x25519-dalek` (key exchange), `chacha20poly1305` (encryption)

## License

MIT
