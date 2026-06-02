# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

TinyVPN — a minimal mesh VPN built in Rust, inspired by Tailscale/ZeroTier. Feature-complete: node registration, WireGuard tunnel setup, NAT hole punching, UDP relay fallback, session authentication, heartbeat, QUIC transport, SQLite persistence, group-based ACL, TCP port forwarding, and Web management dashboard.

## Build & Run

```bash
cargo build                  # build all crates
cargo build -p tinyvpn-ccs   # build single crate
cargo run -p tinyvpn-ccs     # start control server (default 0.0.0.0:9090, override with CCS_ADDR env)
cargo run -p tinyvpn-relay   # start relay server (default 0.0.0.0:9091, override with RELAY_ADDR env)
cargo run -p tinyvpn-cli -- register --name node-a   # register a node
cargo run -p tinyvpn-cli -- connect                  # connect to mesh
cargo run -p tinyvpn-cli -- status                   # show peers
cargo run -p tinyvpn-cli -- disconnect               # tear down WireGuard
cargo run -p tinyvpn-cli -- forward --vpn-ip 10.13.0.2 --remote-port 80 --local-port 8080  # TCP port forward
cargo run -p tinyvpn-cli -- acl --action list        # list ACL groups/rules
cargo test                   # run all tests (32 tests)
cargo test -p tinyvpn-core   # run tests for single crate
cargo clippy --workspace     # lint
```

## Architecture

Rust workspace with 5 crates:

| Crate | Purpose |
|---|---|
| `tinyvpn-core` | Shared library: crypto (x25519 keygen, ChaCha20-Poly1305 encrypt/decrypt), protocol types (`ControlMessage` enum with session auth, `PeerInfo`), config (`NodeConfig` with session_token, persisted at `~/.tinyvpn/config.json`), TLS helpers (`create_server`/`create_client` for QUIC self-signed certs), WireGuard interface management (`WgInterface` via wg-quick) |
| `tinyvpn-ccs` | Control Coordination Server: SQLite-backed node registry, IP assignment from `10.13.0.0/16` with recycling, peer discovery (ACL-filtered), heartbeat tracking (60s timeout), stale node reaper. QUIC transport + newline-delimited JSON. Web dashboard on port 8080 |
| `tinyvpn-p2p` | P2P engine: STUN public endpoint discovery (raw RFC 5389 binding request to Google STUN), UDP hole punching (`Puncher` sends 10 probe packets) |
| `tinyvpn-relay` | Relay server: UDP packet forwarding when hole punching fails. Bidirectional session mapping with 30s timeout |
| `tinyvpn-cli` | CLI client: `register`, `connect`, `status`, `disconnect`, `forward`, `acl` subcommands via clap |

### Data flow

1. Node generates X25519 keypair, sends `Register` to CCS over QUIC
2. CCS assigns VPN IP (sequential from `10.13.0.1`), returns `RegisterOk` with session_token
3. On `connect`, node establishes persistent QUIC connection to CCS, does STUN to discover public endpoint, reports it to CCS
4. Node fetches peer list (authenticated, ACL-filtered), sets up WireGuard interface via wg-quick
5. For each peer: attempts UDP hole punching → success: direct WireGuard peer → failure: request relay, add peer with relay endpoint
6. Heartbeat loop sends `Ping` every 30s; CCS marks nodes offline after 60s without heartbeat
7. On Ctrl+C: teardown WireGuard, disconnect
8. Web dashboard on `0.0.0.0:8080` shows nodes, groups, and ACL rules; auto-refreshes every 5s

### Key constants

- Network CIDR: `10.13.0.0/16` (in `core/src/config.rs`)
- CCS default: `0.0.0.0:9090` (overridable via `CCS_ADDR` env)
- Relay default: `0.0.0.0:9091` (overridable via `RELAY_ADDR` env)
- Web dashboard: `0.0.0.0:8080` (overridable via `WEB_ADDR` env)
- STUN servers: Google STUN (in `core/src/config.rs`)
- WireGuard interface: `wg0`, listen port `51820`

## Known gaps

- TLS uses self-signed certs (no proper CA/PKI)
- No NAT type detection (symmetric NAT blocks hole punching)
- No WireGuard key rotation
