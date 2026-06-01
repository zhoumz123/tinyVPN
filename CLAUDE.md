# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

TinyVPN — a minimal mesh VPN built in Rust, inspired by Tailscale/ZeroTier. Currently at early MVP stage: node registration and STUN-based NAT discovery work; WireGuard tunnel setup and relay fallback are not yet implemented.

## Build & Run

```bash
cargo build                  # build all crates
cargo build -p tinyvpn-ccs   # build single crate
cargo run -p tinyvpn-ccs     # start control server (default 0.0.0.0:9090, override with CCS_ADDR env)
cargo run -p tinyvpn-cli -- register --name node-a   # register a node
cargo run -p tinyvpn-cli -- connect                  # connect to mesh
cargo run -p tinyvpn-cli -- status                   # show peers
cargo test                   # run all tests (none exist yet)
cargo test -p tinyvpn-core   # run tests for single crate
cargo clippy --workspace     # lint
```

## Architecture

Rust workspace with 4 crates:

| Crate | Purpose |
|---|---|
| `tinyvpn-core` | Shared library: crypto (x25519 keygen, ChaCha20-Poly1305 encrypt/decrypt), protocol types (`ControlMessage` enum, `PeerInfo`), config (`NodeConfig` persisted at `~/.tinyvpn/config.json`) |
| `tinyvpn-ccs` | Control Coordination Server: node registry (in-memory `HashMap`), IP assignment from `10.13.0.0/16`, peer discovery. Currently uses TCP + newline-delimited JSON, not the planned QUIC |
| `tinyvpn-p2p` | P2P engine: STUN public endpoint discovery (raw RFC 5389 binding request to Google STUN), UDP hole punching (`Puncher` sends 10 probe packets) |
| `tinyvpn-cli` | CLI client: `register`, `connect`, `status` subcommands via clap |

### Data flow

1. Node generates X25519 keypair, sends `Register` to CCS over TCP
2. CCS assigns VPN IP (sequential from `10.13.0.1`), returns `RegisterOk`
3. On `connect`, node does STUN to discover public endpoint, reports it to CCS
4. Node fetches peer list, attempts UDP hole punching with each peer that has a known endpoint
5. If punching fails after 10 attempts, bail (relay not implemented yet)

### Key constants

- Network CIDR: `10.13.0.0/16` (in `core/src/config.rs`)
- CCS default: `0.0.0.0:9090` (overridable via `CCS_ADDR` env)
- STUN servers: Google STUN (in `core/src/config.rs`)

## Known gaps (from MVP checklist)

- WireGuard tunnel interface not wired up yet
- No authentication on CCS connections
- `UpdateEndpoint` doesn't track which node sent it
- No relay fallback
- No tests
