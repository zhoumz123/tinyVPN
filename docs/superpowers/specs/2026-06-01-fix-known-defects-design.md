# Fix Known MVP Defects — Design Spec

Date: 2026-06-01

## Goal

Fix the three high-severity known defects in the TinyVPN MVP:

1. **Heartbeat not sent** — nodes are reaped as stale after 60s even while connected
2. **Relay session not registered** — relay fallback is non-functional, packets dropped
3. **Zero tests** — no test coverage anywhere in the codebase

## 1. Heartbeat Fix — Shared Writer

### Problem

In `cli/main.rs`, `connect()` splits the TCP stream via `tokio::io::split()`. The `WriteHalf` is consumed by the main loop's `read_ccs_response()`, so the spawned heartbeat task cannot write `Ping` messages. The current heartbeat body is a no-op loop.

### Solution

Wrap `WriteHalf` in `Arc<Mutex<WriteHalf>>` so both the heartbeat task and the main loop can write through cloned `Arc` handles.

### Changes

**`crates/cli/src/main.rs`**:
- After `tokio::io::split()`, wrap `WriteHalf` in `Arc::new(Mutex::new(write_half))`
- Clone the `Arc` into the heartbeat `tokio::spawn` task
- Heartbeat loop: every 30s, lock the writer, serialize `Ping { node_id, session_token }`, write + flush
- Main loop also writes through the shared writer (for `UpdateEndpoint`, `GetPeers`, `RequestRelay`)
- Remove the unused `PunchRequest` match arm in the response handler (CCS never sends it)

### Flow After Fix

```
connect():
  TCP stream → split()
  Arc<Mutex<WriteHalf>> ──clone──→ heartbeat task (Ping every 30s)
  BufReader<ReadHalf>  ──────────→ main loop (read responses)
  main loop also uses Arc<Mutex<WriteHalf>> for sending messages
```

## 2. Relay Session Registration — Node-Direct Registration

### Problem

When hole punching fails, the node receives a relay address from CCS and uses it as the WireGuard peer endpoint. But the relay has no session mapping for that address — it drops all packets from unknown sources. `Relay::register_session()` exists but is never called.

### Solution

Nodes register directly with the relay via a lightweight UDP protocol. Both nodes in a pair must register before the relay establishes bidirectional forwarding.

### Registration Protocol

```
Node → Relay:  "REGISTER:<my_node_id>:<peer_node_id>"
Relay → Node:  "OK"
```

The relay maintains a pending registration map. When both sides of a pair have registered, it calls `register_session()` to establish bidirectional forwarding.

### Changes

**`crates/core/src/protocol.rs`**:
- Add `target_id: Option<String>` field to `RelayAssigned` variant

**`crates/ccs/src/server.rs`**:
- When handling `RequestRelay`, include `target_id` in the `RelayAssigned` response

**`crates/relay/src/lib.rs`**:
- New struct field: `pending: Arc<Mutex<HashMap<(String, String), Vec<SocketAddr>>>>` — maps `(node_id, peer_id)` to registered addresses
- In the main receive loop: detect `REGISTER:` prefix, parse `<my_id>:<peer_id>`, record sender address
- When both `(A, B)` and `(B, A)` have registered, call `register_session(addr_a, addr_b)` and send `OK` to both
- If only one side registered, store it and reply `OK` (peer hasn't registered yet)
- Existing forwarding logic unchanged

**`crates/cli/src/main.rs`**:
- After receiving `RelayAssigned { relay_addr, target_id }`, send UDP `REGISTER:<my_id>:<peer_id>` to relay, wait for `OK` (with timeout)
- Then proceed to `wg.add_peer()` with relay endpoint as before

### Flow After Fix

```
Node A punch fails → RequestRelay(target_id=B) → CCS
CCS → RelayAssigned { relay_addr, target_id: Some(B) } → Node A
Node A → REGISTER:<A_id>:<B_id> → Relay  (stored as pending)
... later, Node B also connects ...
Node B → REGISTER:<B_id>:<A_id> → Relay
Relay sees pair complete → register_session(A_addr, B_addr) → "OK" to both
Now WireGuard packets flow through relay in both directions
```

## 3. Test Coverage

### Strategy

Unit tests in each crate, inline `#[cfg(test)] mod tests` following Rust convention. No new test files. No end-to-end integration tests (require root + WireGuard + network).

### `tinyvpn-core`

- **`crypto`**: encrypt → decrypt roundtrip for empty, small, and large plaintexts; different keys produce different ciphertexts
- **`protocol`**: serialize/deserialize roundtrip for all `ControlMessage` variants; verify JSON field names
- **`config`**: save → load roundtrip with temp dir; `exists()` returns false for nonexistent config

### `tinyvpn-ccs`

- **`registry`**: sequential IP assignment; session validation (valid/invalid token); heartbeat refreshes stale time; `reap_stale()` removes nodes idle > 60s; `get_peers()` filters offline nodes; `update_endpoint()` updates peer info
- **`server`**: start TCP listener, send `Register` → receive `RegisterOk`; send `GetPeers` with valid session → `PeerList`; send `GetPeers` with invalid session → `Pong` (rejected); send `Ping` with valid session → `Pong`

### `tinyvpn-p2p`

- **`puncher`**: create two local UDP sockets, exchange `PUNCH:` packets to verify format and detection logic. This tests the protocol parsing without real NAT traversal.

### `tinyvpn-relay`

- **UDP loopback**: start relay on localhost, register two node pairs via `REGISTER:` protocol, send packets both ways, verify forwarding
- **Timeout cleanup**: register a pair, wait > 30s (or mock time), verify session reaped

## Scope Exclusions

- QUIC transport upgrade
- TLS/encryption on control plane
- CCS persistence
- IP address reclamation
- Removing unused `PunchRequest` variant (low risk, can clean later)
- Replacing hand-rolled base64
