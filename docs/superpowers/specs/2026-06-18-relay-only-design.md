# Design: Relay-Only Mode (Remove P2P Hole-Punching)

**Date:** 2026-06-18
**Status:** Approved
**Approach:** A — minimal change (cli connect flow + delete p2p crate; CCS untouched)

## Goal

Remove the P2P UDP hole-punching (direct-connect) feature entirely. Keep **only** the UDP relay as the transport between peers. Simplify the client connect flow accordingly and delete the now-unused `tinyvpn-p2p` crate.

## Motivation

- Hole-punching has consistently failed in real deployments (restrictive/symmetric NAT), always falling back to relay anyway.
- Relay is the path that actually carries traffic; punching is dead weight that adds STUN dependency, latency, and complexity.
- Reduces moving parts for diagnosis and maintenance.

## Background — current connect flow (to be changed)

`crates/cli/src/main.rs::connect()` today:

1. QUIC-connect to CCS
2. `tinyvpn_p2p::discover_public_endpoint()` (STUN) → `UpdateEndpoint` to CCS
3. `GetPeers` → peer list
4. Setup WireGuard interface
5. For each peer: `Puncher::punch(peer.endpoint)` → on success, direct WG peer; on failure, `RequestRelay` → `register_with_relay` → WG peer with relay endpoint
6. Heartbeat loop (Ping every 30s)

## New connect flow (relay-only)

1. QUIC-connect to CCS
2. `GetPeers` → peer list
3. Setup WireGuard interface
4. For each peer (directly, no punch attempt):
   - `RequestRelay { target_id: peer.node_id }` → `RelayAssigned { relay_addr, target_id }`
   - `register_with_relay(relay_addr, my_id, target_id)`
   - `wg.add_peer(peer.public_key, "peer.vpn_ip/32", Some(relay_addr))`
5. Heartbeat loop (unchanged)

Removed from connect: STUN discovery, `UpdateEndpoint` send, `Puncher` usage, the punch-success direct-endpoint branch.

## Changes

### `crates/cli/src/main.rs`
- Delete STUN block (`discover_public_endpoint` + `UpdateEndpoint`).
- Delete the `Puncher` block and the punch-success/failure branch.
- Per-peer body becomes the relay-only steps above.
- Remove `use` of anything from `tinyvpn_p2p`.

### Delete `crates/p2p/` crate
- Remove `crates/p2p` from `Cargo.toml` `[workspace] members`.
- Remove `tinyvpn-p2p = { path = "crates/p2p" }` from `[workspace.dependencies]`.
- Remove `tinyvpn-p2p` from `crates/cli/Cargo.toml`.
- Delete the `crates/p2p/` directory.

### `crates/core/src/config.rs`
- Remove `STUN_SERVERS` constant (only consumed by the deleted p2p crate).

### CCS — unchanged
- `UpdateEndpoint` handler in `server.rs` stays (cli no longer sends it; becomes harmless dead code). relay protocol unchanged.
- Deferred cleanup (removing the dead handler/protocol variant/`endpoint` field) is out of scope for this change.

## Relay behavior (unchanged, for reference)
- Relay attributes packets by source IP and learns each node's real WireGuard endpoint from observed traffic (port roaming), then fan-outs to all registered peers.
- Both endpoints must send at least one WG handshake to the relay for the relay to learn their endpoints; WireGuard's automatic handshake retries bootstrap this within seconds.

## Testing
- Relay unit tests (`crates/relay/src/lib.rs`) remain — they verify the relay logic end-to-end with real UDP sockets, including port learning and fan-out.
- No p2p tests to carry over (p2p crate deleted).
- Expected post-change test count: 33 (core 15, ccs 15, relay 3).
- Verification: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace`.

## Out of scope
- Fixing the current live connectivity issue (relay not forwarding) — separate diagnosis via relay debug logs.
- CCS `UpdateEndpoint` / `endpoint` field cleanup.
- Direct-connect re-enablement.

## Risks
- Deleting a workspace crate touches `Cargo.toml` + lockfile; rebuild will re-resolve. Low risk.
- If a deployment ever relied on direct P2P (punch success), it now always relays — acceptable given punch reliably failed in practice.
