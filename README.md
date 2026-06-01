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
| `tinyvpn-cli` | CLI client — register, connect, status |
| `tinyvpn-core` | Shared library — crypto, protocol, WireGuard interface management |
| `tinyvpn-p2p` | Peer-to-peer engine — NAT traversal, hole punching, relay fallback |

### MVP Scope

- [x] Project structure
- [ ] Two nodes communicate via WireGuard tunnel
- [ ] Control server coordinates peer discovery & key exchange
- [ ] Basic NAT hole punching (UDP)
- [ ] Relay fallback when P2P fails
- [ ] CLI: `register`, `connect`, `status`

### Quick Start

```bash
# Build everything
cargo build

# Start control server
cargo run -p tinyvpn-ccs

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
