# TinyVPN

A lightweight, open-source mesh VPN built in Rust, inspired by [Tailscale](https://tailscale.com), [ZeroTier](https://www.zerotier.com), and [CloudNet](https://cloudnet.world).

TinyVPN connects distributed devices into a secure virtual network using WireGuard tunnels. Devices communicate directly via peer-to-peer connections with automatic NAT traversal, falling back to a relay server when direct connections are not possible.

## Features

- **Mesh Networking** — All nodes connect directly to each other, no centralized routing
- **NAT Traversal** — STUN-based UDP hole punching for peer-to-peer connectivity
- **Relay Fallback** — Automatic traffic relay when hole punching fails
- **WireGuard Encryption** — X25519 key exchange + ChaCha20-Poly1305 authenticated encryption
- **Session Authentication** — Token-based auth between nodes and the control server
- **Heartbeat & Health** — Automatic online/offline detection with 60s heartbeat timeout
- **Simple Deployment** — One control server + one relay + CLI clients

## Architecture

```
                    ┌──────────────┐
                    │  CCS Server  │ :9090/TCP
                    │  (Control)   │
                    └──────┬───────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
        ┌─────┴─────┐ ┌───┴─────┐ ┌───┴─────┐
        │  Node A   │ │ Node B  │ │ Node C  │
        │ 10.13.0.1 │ │10.13.0.2│ │10.13.0.3│
        └─────┬─────┘ └────┬────┘ └─────────┘
              │             │
              └── P2P or ───┘  Relay :9091/UDP
```

### Components

| Crate | Description |
|-------|-------------|
| `tinyvpn-ccs` | Control Coordination Server — node registration, key exchange, peer discovery, topology management |
| `tinyvpn-relay` | Relay Server — UDP packet forwarding when direct P2P connection fails |
| `tinyvpn-cli` | CLI Client — `register`, `connect`, `status`, `disconnect` commands |
| `tinyvpn-core` | Shared Library — crypto (X25519, ChaCha20-Poly1305), protocol types, WireGuard interface management |
| `tinyvpn-p2p` | P2P Engine — STUN public endpoint discovery, UDP hole punching |

### Connection Flow

1. Node generates X25519 keypair and registers with CCS → receives VPN IP + session token
2. Node connects to CCS via persistent TCP, discovers public endpoint via STUN
3. Node fetches peer list, creates WireGuard interface
4. For each peer: attempts UDP hole punching → direct connection, or falls back to relay
5. Heartbeat loop keeps node online (60s timeout → marked offline)

## Quick Start

### Prerequisites

- Linux with kernel ≥ 5.6 (WireGuard module)
- `wireguard-tools` package (`apt install wireguard-tools`)
- Root privileges (required for WireGuard interface management)

### Build from Source

```bash
git clone https://github.com/zhoumz123/tinyvpn.git
cd tinyvpn
cargo build --release
```

Binaries are in `target/release/`.

### Run

**1. Start the control server and relay:**

```bash
# On your server (e.g. 1.2.3.4)
CCS_ADDR=0.0.0.0:9090 RELAY_ADDR=1.2.3.4:9091 ./target/release/tinyvpn-ccs &
RELAY_ADDR=0.0.0.0:9091 ./target/release/tinyvpn-relay &
```

**2. Register and connect nodes:**

```bash
# On Node A
./target/release/tinyvpn-cli --ccs 1.2.3.4:9090 register --name office
./target/release/tinyvpn-cli --ccs 1.2.3.4:9090 connect
# → VPN IP: 10.13.0.1

# On Node B
./target/release/tinyvpn-cli --ccs 1.2.3.4:9090 register --name home
./target/release/tinyvpn-cli --ccs 1.2.3.4:9090 connect
# → VPN IP: 10.13.0.2
```

**3. Verify connectivity:**

```bash
# On Node A
ping 10.13.0.2
```

### CLI Reference

| Command | Description |
|---------|-------------|
| `register --name <name>` | Register this node (once per machine) |
| `connect` | Connect to the mesh network |
| `status` | Show peer list and connection status |
| `disconnect` | Tear down WireGuard interface and go offline |

| Flag | Default | Description |
|------|---------|-------------|
| `--ccs` | `127.0.0.1:9090` | CCS server address |
| `--interface` | `wg-tinyvpn` | WireGuard interface name |
| `--port` | `51820` | WireGuard UDP listen port |

### Pre-built Release

Download the latest release package:

```bash
tar xzf tinyvpn-0.1.0-linux-aarch64.tar.gz
cd tinyvpn-0.1.0-linux-aarch64

# One-click server start
./scripts/start-ccs.sh

# Client
./bin/tinyvpn-cli --ccs <server-ip>:9090 register --name my-node
./bin/tinyvpn-cli --ccs <server-ip>:9090 connect
```

## Network Ports

| Service | Protocol | Port | Purpose |
|---------|----------|------|---------|
| CCS | TCP | 9090 | Control plane communication |
| Relay | UDP | 9091 | Traffic relay for failed P2P |
| WireGuard | UDP | 51820 | VPN tunnel data (configurable) |

VPN subnet: `10.13.0.0/16` (up to 65,534 nodes)

## Tech Stack

- **Language:** Rust
- **Async Runtime:** Tokio
- **VPN Tunnel:** WireGuard (Linux kernel module)
- **NAT Traversal:** STUN (RFC 5389)
- **Crypto:** X25519 (key exchange), ChaCha20-Poly1305 (encryption)
- **Control Protocol:** TCP + newline-delimited JSON

## Documentation

- [Product User Guide](docs/user-guide.md) — Complete usage manual (Chinese)
- [Deployment Guide](docs/deployment.md) — Server deployment instructions (Chinese)

## Project Status

MVP complete. Implemented:

- [x] WireGuard tunnel between two nodes
- [x] Control server with session authentication
- [x] STUN-based NAT traversal and UDP hole punching
- [x] Relay fallback for failed P2P connections
- [x] CLI: register, connect, status, disconnect

Planned:

- [ ] QUIC transport for control plane
- [ ] TLS encryption for control protocol
- [ ] TCP/UDP port forwarding (内网穿透)
- [ ] ACL / zero-trust policy engine
- [ ] Web management panel
- [ ] Cross-platform clients (macOS, Windows, Android)

## License

MIT
