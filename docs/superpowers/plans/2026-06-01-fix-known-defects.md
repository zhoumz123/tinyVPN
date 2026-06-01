# Fix Known MVP Defects — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the three high-severity MVP defects: broken heartbeat, non-functional relay fallback, and zero test coverage.

**Architecture:** Minimal changes to existing crates. Heartbeat fix uses `Arc<Mutex<WriteHalf>>` sharing. Relay adds a UDP registration protocol where nodes register directly. Tests added inline as `#[cfg(test)]` modules.

**Tech Stack:** Rust, Tokio, serde_json, x25519-dalek, chacha20poly1305

---

## File Structure

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `crates/core/src/protocol.rs` | Add `target_id` to `RelayAssigned` |
| Modify | `crates/cli/src/main.rs` | Shared writer heartbeat + relay registration |
| Modify | `crates/ccs/src/server.rs` | Include `target_id` in `RelayAssigned` response |
| Modify | `crates/relay/src/lib.rs` | UDP registration protocol + session auto-pairing |
| Modify | `crates/core/src/crypto.rs` | Add `#[cfg(test)]` module |
| Modify | `crates/core/src/protocol.rs` | Add `#[cfg(test)]` module |
| Modify | `crates/core/src/config.rs` | Add `#[cfg(test)]` module |
| Modify | `crates/ccs/src/registry.rs` | Add `#[cfg(test)]` module |

---

### Task 1: Update `RelayAssigned` protocol with `target_id`

**Files:**
- Modify: `crates/core/src/protocol.rs:44-47`

The `RelayAssigned` variant currently only has `relay_addr`. Nodes need `target_id` to know which peer they're registering with the relay.

- [ ] **Step 1: Update `RelayAssigned` variant**

In `crates/core/src/protocol.rs`, replace the `RelayAssigned` variant:

```rust
    /// CCS → Node: Relay address assigned
    RelayAssigned {
        relay_addr: String,
        target_id: Option<String>,
    },
```

- [ ] **Step 2: Build workspace to see what breaks**

Run: `cargo build 2>&1`
Expected: FAIL — `server.rs` and `main.rs` construct `RelayAssigned` without `target_id`. We fix these in Tasks 2 and 3.

---

### Task 2: Fix CLI heartbeat with shared writer

**Files:**
- Modify: `crates/cli/src/main.rs:123-256`

- [ ] **Step 1: Add `Arc<Mutex>` import**

At the top of `crates/cli/src/main.rs` (line 1), add imports:

```rust
use std::sync::Arc;
use tokio::sync::Mutex;
```

- [ ] **Step 2: Wrap writer in `Arc<Mutex>` and update `send_on` calls**

Replace the entire `connect` function (lines 123-256) with:

```rust
async fn connect(ccs_addr: &str, wg_interface: &str, wg_port: u16) -> Result<()> {
    use tokio::io::AsyncBufReadExt;

    let config = NodeConfig::load().map_err(|_| {
        anyhow::anyhow!("Not registered yet. Run: tinyvpn register --name <name>")
    })?;

    println!("Connecting as {} ({})...", config.name, config.vpn_ip);

    // Step 1: Establish persistent TCP to CCS
    let stream = tokio::net::TcpStream::connect(ccs_addr).await?;
    let (ccs_reader, ccs_writer) = tokio::io::split(stream);
    let mut lines = tokio::io::BufReader::new(ccs_reader).lines();
    let writer = Arc::new(Mutex::new(ccs_writer));

    // Step 2: STUN discover public endpoint
    println!("Discovering public endpoint via STUN...");
    match tinyvpn_p2p::discover_public_endpoint().await {
        Ok(endpoint) => {
            println!("   Public endpoint: {}", endpoint);
            let msg = ControlMessage::UpdateEndpoint {
                node_id: config.node_id.clone(),
                session_token: config.session_token.clone(),
                public_addr: endpoint.to_string(),
            };
            send_shared(&writer, &msg).await?;
            let _ = lines.next_line().await?;
        }
        Err(e) => {
            println!("   STUN failed: {} (will rely on relay)", e);
        }
    }

    // Step 3: Get peer list
    println!("Fetching peer list...");
    let msg = ControlMessage::GetPeers {
        node_id: config.node_id.clone(),
        session_token: config.session_token.clone(),
    };
    send_shared(&writer, &msg).await?;

    let peer_line = lines.next_line().await?.ok_or_else(|| anyhow::anyhow!("CCS disconnected"))?;
    let peers: Vec<tinyvpn_core::protocol::PeerInfo> = match serde_json::from_str::<ControlMessage>(&peer_line)? {
        ControlMessage::PeerList { peers } => peers,
        other => anyhow::bail!("Unexpected response: {:?}", other),
    };

    if peers.is_empty() {
        println!("   No other peers in the network yet.");
    } else {
        println!("   Found {} peer(s):", peers.len());
        for peer in &peers {
            println!(
                "   - {} ({}) [{}]",
                peer.name,
                peer.vpn_ip,
                if peer.connected { "online" } else { "offline" }
            );
        }
    }

    // Step 4: Setup WireGuard interface
    println!("Setting up WireGuard interface {}...", wg_interface);
    let wg = WgInterface::new(wg_interface);
    wg.setup(&config.vpn_ip, &config.private_key, wg_port)?;

    // Step 5: Connect to each peer
    for peer in &peers {
        if peer.endpoint.is_empty() {
            println!("   Skipping {} (no public endpoint yet)", peer.name);
            continue;
        }

        println!("   Punching to {}...", peer.name);
        let puncher = tinyvpn_p2p::Puncher::new().await?;
        let endpoint: std::net::SocketAddr = peer.endpoint.parse()?;

        let peer_endpoint = match puncher.punch(endpoint, &config.node_id).await {
            Ok(()) => {
                println!("   Connected to {} (direct)", peer.name);
                peer.endpoint.clone()
            }
            Err(_) => {
                println!("   Punch failed, requesting relay for {}...", peer.name);
                let relay_msg = ControlMessage::RequestRelay {
                    node_id: config.node_id.clone(),
                    session_token: config.session_token.clone(),
                    target_id: peer.node_id.clone(),
                };
                send_shared(&writer, &relay_msg).await?;
                let relay_line = lines.next_line().await?.ok_or_else(|| anyhow::anyhow!("CCS disconnected"))?;
                match serde_json::from_str::<ControlMessage>(&relay_line)? {
                    ControlMessage::RelayAssigned { relay_addr, target_id } => {
                        println!("   Using relay {} for {}", relay_addr, peer.name);

                        // Register with relay so it knows our pair
                        if let Some(tid) = target_id {
                            if let Err(e) = register_with_relay(&relay_addr, &config.node_id, &tid).await {
                                println!("   Warning: relay registration failed: {}", e);
                            }
                        }

                        relay_addr
                    }
                    _ => {
                        println!("   Failed to get relay for {}, skipping", peer.name);
                        continue;
                    }
                }
            }
        };

        // Add WireGuard peer
        let allowed_ip = format!("{}/32", peer.vpn_ip);
        if let Err(e) = wg.add_peer(&peer.public_key, &allowed_ip, Some(&peer_endpoint)) {
            println!("   Failed to add WG peer {}: {}", peer.name, e);
        }
    }

    // Step 6: Heartbeat loop + wait for Ctrl+C
    println!("TinyVPN is running. Press Ctrl+C to stop.");

    let node_id = config.node_id.clone();
    let session_token = config.session_token.clone();
    let hb_writer = writer.clone();

    let heartbeat = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let msg = ControlMessage::Ping {
                node_id: node_id.clone(),
                session_token: session_token.clone(),
            };
            if let Ok(data) = serde_json::to_string(&msg) {
                let mut w = hb_writer.lock().await;
                use tokio::io::AsyncWriteExt;
                if w.write_all(data.as_bytes()).await.is_err()
                    || w.write_all(b"\n").await.is_err()
                {
                    break;
                }
                let _ = w.flush().await;
            }
        }
    });

    tokio::signal::ctrl_c().await?;
    println!("\nShutting down...");
    heartbeat.abort();
    let _ = wg.teardown();
    println!("Bye!");
    Ok(())
}
```

- [ ] **Step 3: Add `send_shared` and `register_with_relay` helper functions**

Add these after the existing `send_on` function (after line 337):

```rust
/// Send a message on a shared (Arc<Mutex>) writer
async fn send_shared(
    writer: &Arc<tokio::sync::Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>,
    msg: &ControlMessage,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut w = writer.lock().await;
    let data = serde_json::to_string(msg)?;
    w.write_all(data.as_bytes()).await?;
    w.write_all(b"\n").await?;
    w.flush().await?;
    Ok(())
}

/// Register this node with the relay server for a given peer pair
async fn register_with_relay(relay_addr: &str, my_id: &str, peer_id: &str) -> Result<()> {
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    let relay: std::net::SocketAddr = relay_addr.parse()?;
    let msg = format!("REGISTER:{}:{}", my_id, peer_id);
    socket.send_to(msg.as_bytes(), relay).await?;

    let mut buf = [0u8; 64];
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.recv_from(&mut buf),
    )
    .await
    {
        Ok(Ok((n, _))) => {
            let resp = String::from_utf8_lossy(&buf[..n]);
            if resp.starts_with("OK") {
                Ok(())
            } else {
                anyhow::bail!("Relay rejected registration: {}", resp)
            }
        }
        Ok(Err(e)) => Err(e.into()),
        Err(_) => anyhow::bail!("Relay registration timed out"),
    }
}
```

- [ ] **Step 4: Remove old `send_on` function**

Delete the `send_on` function (lines 327-337) since `send_shared` replaces it. The `register` and `status` functions use `send_to_ccs` (one-shot), not `send_on`, so only `connect` was using it.

- [ ] **Step 5: Build to check compilation**

Run: `cargo build -p tinyvpn-cli 2>&1`
Expected: FAIL — `RelayAssigned` pattern needs `target_id`. We fix this after CCS is updated. For now verify the shared-writer and `register_with_relay` code compiles structurally.

---

### Task 3: Update CCS to include `target_id` in `RelayAssigned`

**Files:**
- Modify: `crates/ccs/src/server.rs:88-102`

- [ ] **Step 1: Update `RequestRelay` handler**

In `crates/ccs/src/server.rs`, replace the `RequestRelay` match arm (lines 88-102):

```rust
            ControlMessage::RequestRelay {
                node_id,
                session_token,
                target_id,
            } => {
                let reg = registry.read().await;
                if reg.validate_session(&node_id, &session_token) {
                    let relay_addr = reg.relay_addr().to_string();
                    serde_json::to_string(&ControlMessage::RelayAssigned {
                        relay_addr,
                        target_id: Some(target_id),
                    })?
                } else {
                    serde_json::to_string(&ControlMessage::Pong)?
                }
            }
```

- [ ] **Step 2: Build CCS**

Run: `cargo build -p tinyvpn-ccs`
Expected: PASS

---

### Task 4: Add registration protocol to relay

**Files:**
- Modify: `crates/relay/src/lib.rs`

This is the core relay fix. The relay must accept `REGISTER:<my_id>:<peer_id>` UDP packets and auto-pair sessions when both sides register.

- [ ] **Step 1: Rewrite `crates/relay/src/lib.rs`**

Replace the entire file:

```rust
//! TinyVPN Relay — UDP packet forwarding for punch-failure fallback

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use anyhow::Result;

const SESSION_TIMEOUT_SECS: u64 = 30;
const BUF_SIZE: usize = 65535;

/// Bidirectional session mapping: addr_a <-> addr_b
struct Session {
    peer: SocketAddr,
    last_activity: Instant,
}

/// Pending registration: node_id waiting for its peer to also register
struct PendingReg {
    addr: SocketAddr,
    peer_id: String,
}

pub struct Relay {
    socket: Arc<UdpSocket>,
    sessions: Arc<Mutex<HashMap<SocketAddr, Session>>>,
    /// (my_node_id, peer_node_id) → registered address
    pending: Arc<Mutex<HashMap<(String, String), PendingReg>>>,
}

impl Relay {
    pub async fn bind(addr: &str) -> Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        tracing::info!("Relay listening on {}", socket.local_addr()?);
        Ok(Self {
            socket: Arc::new(socket),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Register a bidirectional session between two addresses
    pub async fn register_session(&self, a: SocketAddr, b: SocketAddr) {
        let mut sessions = self.sessions.lock().await;
        sessions.insert(
            a,
            Session {
                peer: b,
                last_activity: Instant::now(),
            },
        );
        sessions.insert(
            b,
            Session {
                peer: a,
                last_activity: Instant::now(),
            },
        );
        tracing::info!("Registered relay session: {} <-> {}", a, b);
    }

    /// Run the forwarding loop
    pub async fn run(&self) -> Result<()> {
        // Cleanup task
        let sessions = self.sessions.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;
                let mut sessions = sessions.lock().await;
                let now = Instant::now();
                sessions.retain(|addr, session| {
                    let keep = now.duration_since(session.last_activity).as_secs() < SESSION_TIMEOUT_SECS;
                    if !keep {
                        tracing::info!("Reaped stale session for {}", addr);
                    }
                    keep
                });
            }
        });

        // Forwarding + registration loop
        let mut buf = vec![0u8; BUF_SIZE];
        loop {
            let (n, from) = self.socket.recv_from(&mut buf).await?;
            let msg = String::from_utf8_lossy(&buf[..n]);

            if let Some(rest) = msg.strip_prefix("REGISTER:") {
                let parts: Vec<&str> = rest.splitn(2, ':').collect();
                if parts.len() == 2 {
                    let my_id = parts[0].to_string();
                    let peer_id = parts[1].to_string();
                    self.handle_register(from, my_id, peer_id).await;
                }
                // Send OK acknowledgment
                self.socket.send_to(b"OK", from).await?;
                continue;
            }

            // Normal forwarding
            let sessions = self.sessions.lock().await;

            if let Some(_session) = sessions.get(&from) {
                drop(sessions);
                let mut sessions = self.sessions.lock().await;
                if let Some(session) = sessions.get_mut(&from) {
                    session.last_activity = Instant::now();
                }
                drop(sessions);

                let sessions = self.sessions.lock().await;
                if let Some(session) = sessions.get(&from) {
                    let peer = session.peer;
                    drop(sessions);
                    self.socket.send_to(&buf[..n], peer).await?;
                    tracing::trace!("Relayed {} bytes from {} to {}", n, from, peer);
                }
            } else {
                tracing::warn!("Packet from unknown address: {}", from);
            }
        }
    }

    async fn handle_register(&self, from: SocketAddr, my_id: String, peer_id: String) {
        let mut pending = self.pending.lock().await;

        // Check if peer already registered with us as their target
        let pair_key = (peer_id.clone(), my_id.clone());
        if let Some(existing) = pending.remove(&pair_key) {
            // Both sides registered — create bidirectional session
            drop(pending);
            tracing::info!(
                "Pair complete: {} ({}) <-> {} ({})",
                my_id, from, peer_id, existing.addr
            );
            self.register_session(from, existing.addr).await;
            return;
        }

        // Store our registration, wait for peer
        let reverse_key = (my_id.clone(), peer_id.clone());
        pending.insert(
            reverse_key,
            PendingReg {
                addr: from,
                peer_id: peer_id.clone(),
            },
        );
        tracing::info!("Registered {} -> {} (waiting for peer)", my_id, peer_id);
    }
}
```

- [ ] **Step 2: Build relay**

Run: `cargo build -p tinyvpn-relay`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/protocol.rs crates/cli/src/main.rs crates/ccs/src/server.rs crates/relay/src/lib.rs
git commit -m "fix: heartbeat shared writer, relay node-direct registration, RelayAssigned target_id"
```

---

### Task 5: Full workspace build verification

**Files:** None

- [ ] **Step 1: Build full workspace**

Run: `cargo build`
Expected: PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace 2>&1`
Expected: PASS (fix any warnings)

- [ ] **Step 3: Commit any clippy fixes**

```bash
git add -A
git commit -m "fix: address clippy warnings"
```

---

### Task 6: Core unit tests — crypto

**Files:**
- Modify: `crates/core/src/crypto.rs`

- [ ] **Step 1: Add test module to `crates/core/src/crypto.rs`**

Append at end of file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let (secret, _) = generate_keypair();
        let key = secret.to_bytes();
        let nonce = random_nonce();
        let plaintext = b"hello tinyvpn";
        let ciphertext = encrypt(&key, &nonce, plaintext).unwrap();
        let decrypted = decrypt(&key, &nonce, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_decrypt_empty() {
        let (secret, _) = generate_keypair();
        let key = secret.to_bytes();
        let nonce = random_nonce();
        let ciphertext = encrypt(&key, &nonce, b"").unwrap();
        let decrypted = decrypt(&key, &nonce, &ciphertext).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn encrypt_decrypt_large() {
        let (secret, _) = generate_keypair();
        let key = secret.to_bytes();
        let nonce = random_nonce();
        let plaintext = vec![0xAB_u8; 10_000];
        let ciphertext = encrypt(&key, &nonce, &plaintext).unwrap();
        let decrypted = decrypt(&key, &nonce, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let (secret1, _) = generate_keypair();
        let (secret2, _) = generate_keypair();
        let nonce = random_nonce();
        let ciphertext = encrypt(&secret1.to_bytes(), &nonce, b"secret").unwrap();
        assert!(decrypt(&secret2.to_bytes(), &nonce, &ciphertext).is_err());
    }

    #[test]
    fn wrong_nonce_fails() {
        let (secret, _) = generate_keypair();
        let key = secret.to_bytes();
        let nonce1 = random_nonce();
        let nonce2 = random_nonce();
        let ciphertext = encrypt(&key, &nonce1, b"secret").unwrap();
        assert!(decrypt(&key, &nonce2, &ciphertext).is_err());
    }
}
```

- [ ] **Step 2: Run crypto tests**

Run: `cargo test -p tinyvpn-core crypto`
Expected: 5 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/crypto.rs
git commit -m "test(core): add crypto unit tests"
```

---

### Task 7: Core unit tests — protocol serialization

**Files:**
- Modify: `crates/core/src/protocol.rs`

- [ ] **Step 1: Add test module to `crates/core/src/protocol.rs`**

Append at end of file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: ControlMessage) {
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: ControlMessage = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&decoded).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn register_roundtrip() {
        roundtrip(ControlMessage::Register {
            name: "test-node".into(),
            public_key: "abc123".into(),
        });
    }

    #[test]
    fn register_ok_roundtrip() {
        roundtrip(ControlMessage::RegisterOk {
            node_id: "node-abc".into(),
            vpn_ip: "10.13.0.1".into(),
            session_token: "token123".into(),
        });
    }

    #[test]
    fn peer_list_roundtrip() {
        let peers = vec![
            PeerInfo {
                node_id: "n1".into(),
                name: "alpha".into(),
                vpn_ip: "10.13.0.1".into(),
                public_key: "pub1".into(),
                endpoint: "1.2.3.4:51820".into(),
                connected: true,
            },
            PeerInfo {
                node_id: "n2".into(),
                name: "beta".into(),
                vpn_ip: "10.13.0.2".into(),
                public_key: "pub2".into(),
                endpoint: String::new(),
                connected: false,
            },
        ];
        roundtrip(ControlMessage::PeerList { peers });
    }

    #[test]
    fn relay_assigned_with_target() {
        roundtrip(ControlMessage::RelayAssigned {
            relay_addr: "1.2.3.4:9091".into(),
            target_id: Some("node-xyz".into()),
        });
    }

    #[test]
    fn relay_assigned_without_target() {
        roundtrip(ControlMessage::RelayAssigned {
            relay_addr: "1.2.3.4:9091".into(),
            target_id: None,
        });
    }

    #[test]
    fn ping_pong_roundtrip() {
        roundtrip(ControlMessage::Ping {
            node_id: "n1".into(),
            session_token: "tok".into(),
        });
        roundtrip(ControlMessage::Pong);
    }

    #[test]
    fn json_field_names() {
        let msg = ControlMessage::Register {
            name: "x".into(),
            public_key: "y".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"public_key\""));
    }
}
```

- [ ] **Step 2: Run protocol tests**

Run: `cargo test -p tinyvpn-core protocol`
Expected: 7 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/protocol.rs
git commit -m "test(core): add protocol serialization tests"
```

---

### Task 8: Core unit tests — config save/load

**Files:**
- Modify: `crates/core/src/config.rs`

- [ ] **Step 1: Add test module to `crates/core/src/config.rs`**

Append at end of file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> NodeConfig {
        NodeConfig {
            node_id: "node-test".into(),
            name: "test".into(),
            private_key: "privkey123".into(),
            vpn_ip: "10.13.0.5".into(),
            ccs_addr: "127.0.0.1:9090".into(),
            session_token: "tok123".into(),
        }
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join("tinyvpn_test_config");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config = make_config();
        let path = dir.join("config.json");
        let data = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &data).unwrap();

        let loaded: NodeConfig =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(loaded.node_id, config.node_id);
        assert_eq!(loaded.name, config.name);
        assert_eq!(loaded.vpn_ip, config.vpn_ip);
        assert_eq!(loaded.session_token, config.session_token);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_json_has_expected_fields() {
        let config = make_config();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"node_id\""));
        assert!(json.contains("\"vpn_ip\""));
        assert!(json.contains("\"session_token\""));
    }
}
```

- [ ] **Step 2: Run config tests**

Run: `cargo test -p tinyvpn-core config`
Expected: 2 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/config.rs
git commit -m "test(core): add config save/load tests"
```

---

### Task 9: CCS registry unit tests

**Files:**
- Modify: `crates/ccs/src/registry.rs`

- [ ] **Step 1: Add test module to `crates/ccs/src/registry.rs`**

Append at end of file (before the closing — there is no closing brace since `uuid_short` is a free function, append after it):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn new_registry() -> Registry {
        Registry::new("127.0.0.1:9091".into())
    }

    #[test]
    fn register_assigns_sequential_ips() {
        let mut reg = new_registry();
        let (id1, ip1, _) = reg.register("a".into(), "pk1".into());
        let (id2, ip2, _) = reg.register("b".into(), "pk2".into());
        assert!(id1.starts_with("node-"));
        assert!(id2.starts_with("node-"));
        assert_ne!(id1, id2);
        assert_eq!(ip1, "10.13.0.1");
        assert_eq!(ip2, "10.13.0.2");
    }

    #[test]
    fn validate_session_correct() {
        let mut reg = new_registry();
        let (id, _, tok) = reg.register("a".into(), "pk1".into());
        assert!(reg.validate_session(&id, &tok));
    }

    #[test]
    fn validate_session_wrong_token() {
        let mut reg = new_registry();
        let (id, _, _) = reg.register("a".into(), "pk1".into());
        assert!(!reg.validate_session(&id, "wrong"));
    }

    #[test]
    fn validate_session_unknown_node() {
        let reg = new_registry();
        assert!(!reg.validate_session("nonexistent", "any"));
    }

    #[test]
    fn heartbeat_refreshes() {
        let mut reg = new_registry();
        let (id, _, tok) = reg.register("a".into(), "pk1".into());

        // Simulate 61 seconds passing
        let entry = reg.nodes.get_mut(&id).unwrap();
        entry.last_heartbeat = Instant::now() - std::time::Duration::from_secs(61);

        reg.heartbeat(&id);
        let entry = reg.nodes.get(&id).unwrap();
        assert!(entry.info.connected);
        assert!(Instant::now().duration_since(entry.last_heartbeat).as_secs() < 5);
    }

    #[test]
    fn reap_stale_marks_offline() {
        let mut reg = new_registry();
        let (id, _, _) = reg.register("a".into(), "pk1".into());

        // Manually age the heartbeat
        let entry = reg.nodes.get_mut(&id).unwrap();
        entry.last_heartbeat = Instant::now() - std::time::Duration::from_secs(61);

        reg.reap_stale();
        let entry = reg.nodes.get(&id).unwrap();
        assert!(!entry.info.connected);
    }

    #[test]
    fn get_peers_excludes_self() {
        let mut reg = new_registry();
        let (id1, _, _) = reg.register("a".into(), "pk1".into());
        let (id2, _, _) = reg.register("b".into(), "pk2".into());

        let peers = reg.get_peers(Some(&id1));
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].node_id, id2);
    }

    #[test]
    fn update_endpoint() {
        let mut reg = new_registry();
        let (id, _, _) = reg.register("a".into(), "pk1".into());
        reg.update_endpoint(&id, "1.2.3.4:51820".into());
        let peer = reg.get_peer(&id).unwrap();
        assert_eq!(peer.endpoint, "1.2.3.4:51820");
    }

    #[test]
    fn relay_addr() {
        let reg = Registry::new("10.0.0.1:9091".into());
        assert_eq!(reg.relay_addr(), "10.0.0.1:9091");
    }
}
```

- [ ] **Step 2: Run registry tests**

Run: `cargo test -p tinyvpn-ccs registry`
Expected: 9 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/ccs/src/registry.rs
git commit -m "test(ccs): add registry unit tests"
```

---

### Task 10: Relay unit tests — registration and forwarding

**Files:**
- Modify: `crates/relay/src/lib.rs`

- [ ] **Step 1: Add test module to `crates/relay/src/lib.rs`**

Append at end of file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_pair_and_forward() {
        let relay = Relay::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay.socket.local_addr().unwrap();

        // Run relay in background
        let relay_handle = tokio::spawn(async move {
            let _ = relay.run().await;
        });

        // Two nodes register
        let node_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let node_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        // Node A registers: I am A, want to talk to B
        node_a.send_to(b"REGISTER:node-a:node-b", relay_addr).await.unwrap();
        let mut buf = [0u8; 64];
        let (n, _) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            node_a.recv_from(&mut buf),
        ).await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).starts_with("OK"));

        // Node B registers: I am B, want to talk to A
        node_b.send_to(b"REGISTER:node-b:node-a", relay_addr).await.unwrap();
        let (n, _) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            node_b.recv_from(&mut buf),
        ).await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).starts_with("OK"));

        // Now A can send data to B through relay
        node_a.send_to(b"hello from A", relay_addr).await.unwrap();
        let (n, _) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            node_b.recv_from(&mut buf),
        ).await.unwrap().unwrap();
        assert_eq!(&buf[..n], b"hello from A");

        // And B can send to A
        node_b.send_to(b"hello from B", relay_addr).await.unwrap();
        let (n, _) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            node_a.recv_from(&mut buf),
        ).await.unwrap().unwrap();
        assert_eq!(&buf[..n], b"hello from B");

        relay_handle.abort();
    }

    #[tokio::test]
    async fn unknown_packet_dropped() {
        let relay = Relay::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay.socket.local_addr().unwrap();

        let relay_handle = tokio::spawn(async move {
            let _ = relay.run().await;
        });

        let stranger = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        stranger.send_to(b"random data", relay_addr).await.unwrap();

        // Nothing should come back — no session registered
        let mut buf = [0u8; 64];
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            stranger.recv_from(&mut buf),
        ).await;
        assert!(result.is_err()); // timeout = no response

        relay_handle.abort();
    }
}
```

- [ ] **Step 2: Run relay tests**

Run: `cargo test -p tinyvpn-relay`
Expected: 2 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/relay/src/lib.rs
git commit -m "test(relay): add registration and forwarding tests"
```

---

### Task 11: Final verification

**Files:** None

- [ ] **Step 1: Run all tests**

Run: `cargo test --workspace`
Expected: All tests PASS (core: 14, ccs: 9, relay: 2 = 25 total)

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace 2>&1`
Expected: PASS

- [ ] **Step 3: Update CLAUDE.md known gaps**

In `CLAUDE.md`, replace the "Known gaps" section:

```markdown
## Known gaps (post-MVP)

- QUIC transport not implemented (still TCP+JSON)
- No TLS encryption on control plane
- No CCS persistence (in-memory registry lost on restart)
- IP address space not reclaimed (sequential assignment, no reuse)
```

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md — remove fixed defects from known gaps"
```
