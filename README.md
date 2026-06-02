# TinyVPN

A lightweight, open-source mesh VPN built in Rust, inspired by [Tailscale](https://tailscale.com) and [ZeroTier](https://www.zerotier.com).

TinyVPN connects distributed devices into a secure virtual network using WireGuard tunnels. Devices communicate directly via peer-to-peer connections with automatic NAT traversal, falling back to a relay server when direct connections are not possible.

## Features

- **Mesh Networking** — All nodes connect directly to each other, no centralized routing
- **QUIC Transport** — Control plane uses QUIC with TLS, stream-per-request multiplexing
- **NAT Traversal** — STUN-based UDP hole punching for peer-to-peer connectivity
- **Relay Fallback** — Automatic traffic relay when hole punching fails
- **WireGuard Encryption** — X25519 key exchange + ChaCha20-Poly1305 authenticated encryption
- **Session Authentication** — Token-based auth between nodes and the control server
- **Heartbeat & Health** — Automatic online/offline detection with 60s heartbeat timeout
- **SQLite Persistence** — Node registry survives restarts, IP address recycling
- **Group-based ACL** — Fine-grained access control with group policies
- **TCP Port Forwarding** — Expose remote services through the VPN mesh
- **Web Dashboard** — Browser-based management with real-time node status and ACL management

## Architecture

```
                    ┌──────────────┐
                    │  CCS Server  │ :9090/QUIC
                    │  (Control)   │ :38080/HTTP (Dashboard)
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
| `tinyvpn-ccs` | Control Coordination Server — SQLite-backed node registry, peer discovery, ACL, web dashboard |
| `tinyvpn-relay` | Relay Server — UDP packet forwarding when direct P2P connection fails |
| `tinyvpn-cli` | CLI Client — `register`, `connect`, `status`, `disconnect`, `forward`, `acl` commands |
| `tinyvpn-core` | Shared Library — crypto, protocol types, QUIC TLS helpers, WireGuard interface management |
| `tinyvpn-p2p` | P2P Engine — STUN public endpoint discovery, UDP hole punching |

### Connection Flow

1. Node generates X25519 keypair and registers with CCS over QUIC → receives VPN IP + session token
2. Node establishes persistent QUIC connection to CCS, discovers public endpoint via STUN
3. Node fetches peer list (ACL-filtered), creates WireGuard interface
4. For each peer: attempts UDP hole punching → direct connection, or falls back to relay
5. Heartbeat loop keeps node online (60s timeout → marked offline)
6. Web dashboard shows real-time node status and ACL management at `:38080`

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

**4. TCP port forwarding:**

```bash
# Forward local port 8080 to Node B's port 80 via VPN
./target/release/tinyvpn-cli forward --vpn-ip 10.13.0.2 --remote-port 80 --local-port 8080
```

**5. ACL management:**

```bash
# List ACL groups and rules
./target/release/tinyvpn-cli acl --action list

# Add node to group
./target/release/tinyvpn-cli acl --action add-group --node-id <id> --group-name dev

# Add ACL rule: admin group can see dev group
./target/release/tinyvpn-cli acl --action add-rule --from-group admin --to-group dev
```

### CLI Reference

| Command | Description |
|---------|-------------|
| `register --name <name>` | Register this node (once per machine) |
| `connect` | Connect to the mesh network |
| `status` | Show peer list and connection status |
| `disconnect` | Tear down WireGuard interface and go offline |
| `forward` | TCP port forwarding to a remote VPN node |
| `acl` | Manage ACL groups and rules |

| Flag | Default | Description |
|------|---------|-------------|
| `--ccs` | `127.0.0.1:9090` | CCS server address |
| `--interface` | `wg-tinyvpn` | WireGuard interface name |
| `--port` | `51820` | WireGuard UDP listen port |

## Network Ports

| Service | Protocol | Port | Purpose |
|---------|----------|------|---------|
| CCS | QUIC | 9090 | Control plane communication |
| Relay | UDP | 9091 | Traffic relay for failed P2P |
| Web Dashboard | HTTP | 38080 | Management interface |
| WireGuard | UDP | 51820 | VPN tunnel data (configurable) |

VPN subnet: `10.13.0.0/16` (up to 65,534 nodes)

## Tech Stack

- **Language:** Rust
- **Async Runtime:** Tokio
- **Transport:** QUIC (quinn) with self-signed TLS (rustls + rcgen)
- **VPN Tunnel:** WireGuard (Linux kernel module)
- **NAT Traversal:** STUN (RFC 5389)
- **Crypto:** X25519 (key exchange), ChaCha20-Poly1305 (encryption)
- **Persistence:** SQLite (rusqlite)
- **Web:** axum + embedded HTML/JS dashboard
- **CLI:** clap

## Project Status

Feature-complete:

- [x] WireGuard tunnel between two nodes
- [x] Control server with session authentication
- [x] STUN-based NAT traversal and UDP hole punching
- [x] Relay fallback for failed P2P connections
- [x] QUIC transport with TLS for control plane
- [x] Stream-per-request QUIC multiplexing
- [x] SQLite persistence with IP recycling
- [x] Heartbeat and stale node reaper
- [x] Group-based ACL policy engine
- [x] TCP port forwarding
- [x] Web management dashboard
- [x] CLI: register, connect, status, disconnect, forward, acl

Planned:

- [ ] NAT type detection (symmetric NAT handling)
- [ ] WireGuard key rotation
- [ ] Cross-platform clients (macOS, Windows, Android)

## Documentation

- [Product User Guide](docs/user-guide.md) — Complete usage manual (Chinese)
- [Deployment Guide](docs/deployment.md) — Server deployment instructions (Chinese)

## License

MIT
