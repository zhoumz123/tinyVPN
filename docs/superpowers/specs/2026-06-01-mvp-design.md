# TinyVPN MVP Design

## Goal

Complete the MVP: two nodes communicate via WireGuard tunnel, with CCS coordination, NAT hole punching, and UDP relay fallback.

## Approach

Incremental completion on existing codebase. TCP+JSON transport stays. No refactoring of existing modules.

## Architecture

```
┌──────────┐  TCP+JSON  ┌─────┐  TCP+JSON  ┌──────────┐
│  Node A  │◄──────────►│ CCS │◄──────────►│  Node B  │
│  (CLI)   │            └─────┘            │  (CLI)   │
└────┬─────┘                                └────┬─────┘
     │ UDP hole punch                           │ UDP hole punch
     └──────────────────────────────────────────┘
     ╔═══════════ WireGuard Tunnel ══════════════╗

     ┌──────────┐
     │  Relay   │  UDP forwarding when punch fails
     └──────────┘
```

## Module Changes

### 1. WireGuard Interface Management (`core/src/wg.rs` — new file)

Responsible for generating WireGuard config and managing TUN interface via `wg-quick` CLI.

```rust
pub struct WgInterface {
    name: String,        // e.g. "wg0"
    config_dir: PathBuf, // where .conf files live
}

impl WgInterface {
    pub fn setup(&self, our_ip: &str, private_key: &str, listen_port: u16) -> Result<()>
    pub fn add_peer(&self, public_key: &str, allowed_ip: &str, endpoint: Option<&str>) -> Result<()>
    pub fn remove_peer(&self, public_key: &str) -> Result<()>
    pub fn teardown(&self) -> Result<()>
}
```

- `setup`: generates a wg-quick `.conf` file, runs `wg-quick up <conf>`
- `add_peer`: runs `wg set <ifname> peer <pubkey> allowed-ips <ip> [endpoint <addr>]`
- `remove_peer`: runs `wg set <ifname> peer <pubkey> remove`
- `teardown`: runs `wg-quick down <conf>`

### 2. Relay Server (`crates/relay` — new crate)

Simple UDP packet forwarding. No TURN protocol.

```rust
pub struct Relay {
    socket: UdpSocket,
    // bidirectional: addr_a <-> addr_b
    sessions: HashMap<SocketAddr, SocketAddr>,
}
```

- Listens on `0.0.0.0:9091` (configurable via `RELAY_ADDR`)
- CCS informs Relay which node pairs to bridge (via internal TCP or shared config)
- When Relay receives a packet from addr A, looks up the mapped addr B and forwards
- Sessions timeout after 30 seconds of no traffic
- Standalone process: `cargo run -p tinyvpn-relay`

### 3. CCS Enhancements

**Session authentication:**
- `Register` → CCS returns `session_token` (random string) in `RegisterOk`
- All subsequent messages from the node carry `node_id` + `session_token`
- CCS validates token before processing requests

**Updated protocol messages:**
```rust
pub enum ControlMessage {
    Register { name: String, public_key: String },
    RegisterOk { node_id: String, vpn_ip: String, session_token: String },
    GetPeers { node_id: String },
    PeerList { peers: Vec<PeerInfo> },
    UpdateEndpoint { node_id: String, session_token: String, public_addr: String },
    RequestRelay { node_id: String, session_token: String, target_id: String },
    RelayAssigned { relay_addr: String },
    PunchRequest { peer_id: String, peer_public_key: String, peer_endpoint: String },
    Ping,
    Pong,
}
```

**Heartbeat:**
- Nodes send `Ping` every 30 seconds on persistent TCP connection
- CCS marks nodes as `connected: false` after 60 seconds without heartbeat
- Peer list `connected` field becomes meaningful

**Persistent TCP in connect mode:**
- `connect` command holds a single long-lived TCP connection to CCS
- Register and status can still use short-lived connections (one request-response)

### 4. CLI Improvements

**`connect` full flow:**
1. Load NodeConfig (requires prior `register`)
2. Establish persistent TCP to CCS
3. STUN discover public endpoint → `UpdateEndpoint` to CCS
4. `GetPeers` → peer list
5. `WgInterface::setup` — create wg0 interface
6. For each peer with known endpoint:
   - Attempt UDP hole punch
   - Success → `add_peer(endpoint=public_addr)`
   - Failure → `RequestRelay` → `add_peer(endpoint=relay_addr)`
7. Heartbeat loop: send `Ping` every 30s
8. On Ctrl+C: `teardown` WireGuard, disconnect from CCS

**New subcommand:**
- `disconnect` — teardown WireGuard interface, notify CCS

**`register` update:**
- Save `session_token` from `RegisterOk` into NodeConfig

**`status` update:**
- Send `node_id` + `session_token` with `GetPeers`
- Display peer connection type: direct / relay / offline

### 5. Workspace Structure

New crate added to workspace:
```
crates/relay/       — tinyvpn-relay
  src/main.rs       — relay server entry point
  src/lib.rs        — Relay struct and forwarding logic
```

`Cargo.toml` workspace members: add `"crates/relay"`

## Data Flow

### Successful direct connection

```
NodeA                    CCS                    NodeB
  │── Register ──────────►│                      │
  │◄── RegisterOk ────────│                      │
  │                       │◄── Register ─────────│
  │                       │─── RegisterOk ──────►│
  │── connect (TCP) ─────►│◄── connect (TCP) ────│
  │── UpdateEndpoint ────►│◄── UpdateEndpoint ──│
  │── GetPeers ──────────►│                      │
  │◄── PeerList [B] ──────│                      │
  │──── UDP punch ──────────────────────────────►│
  │◄─── UDP punch ───────────────────────────────│
  │  wg add_peer(B, endpoint)                    │
  │◄══════ WireGuard tunnel ═════════════════════►│
```

### Relay fallback

```
NodeA ──RequestRelay──► CCS
      ◄──RelayAssigned──
NodeA ──UDP──► Relay ──UDP──► NodeB
```

## Error Handling

- STUN failure: warn and continue (rely on relay)
- Punch failure: automatic relay fallback, does not interrupt connect
- Relay unavailable: log error, skip that peer, continue with others
- WireGuard operation failure (needs root): clear error message, exit
- CCS unreachable: exit with error, no offline mode

## Testing

- `core`: unit tests for crypto encrypt/decrypt roundtrip, protocol serialization, wg config generation
- `relay`: local UDP loopback test
- `ccs` + `cli`: integration test — start CCS → register two nodes → connect → ping through WireGuard
