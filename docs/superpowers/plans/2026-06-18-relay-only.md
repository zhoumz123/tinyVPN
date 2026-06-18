# Relay-Only Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove P2P UDP hole-punching; the client connects to peers purely via the UDP relay.

**Architecture:** Simplify `cli connect()` to skip STUN/punch and request relay directly for every peer. Delete the now-unused `tinyvpn-p2p` crate and the `STUN_SERVERS` constant. CCS and the relay protocol/logic are unchanged.

**Tech Stack:** Rust workspace (5→4 crates), tokio, quinn, wg-quick.

**Spec:** `docs/superpowers/specs/2026-06-18-relay-only-design.md`

---

## File Structure

- `crates/cli/src/main.rs` — rewrite `connect()` (remove STUN/punch, relay-only peer loop)
- `crates/cli/Cargo.toml` — drop `tinyvpn-p2p` dependency
- `Cargo.toml` (workspace) — remove `crates/p2p` from members, drop `tinyvpn-p2p` from `[workspace.dependencies]`
- `crates/p2p/` — delete entire directory
- `crates/core/src/config.rs` — remove `STUN_SERVERS` constant

No new files. This is a deletion/simplification refactor; the regression guard is the existing test suite (33 tests) plus `cargo build`/`clippy` staying clean.

---

### Task 1: Simplify `connect()` — remove STUN + punch, relay-only peer loop

**Files:**
- Modify: `crates/cli/src/main.rs` (the `connect()` function, roughly lines 155–249)

- [ ] **Step 1: Remove the STUN / UpdateEndpoint block**

In `connect()`, delete the STUN discovery block. Replace:

```rust
    println!("Discovering public endpoint via STUN...");
    match tinyvpn_p2p::discover_public_endpoint().await {
        Ok(stun_endpoint) => {
            println!("   Public endpoint: {}", stun_endpoint);
            conn_rpc(&conn, &ControlMessage::UpdateEndpoint {
                node_id: config.node_id.clone(),
                session_token: config.session_token.clone(),
                public_addr: stun_endpoint.to_string(),
            }).await?;
        }
        Err(e) => {
            println!("   STUN failed: {} (will rely on relay)", e);
        }
    }

    println!("Fetching peer list...");
```

with:

```rust
    println!("Fetching peer list...");
```

- [ ] **Step 2: Rewrite the per-peer loop to relay-only**

Replace the entire `for peer in &peers { ... }` block:

```rust
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
                let resp = conn_rpc(&conn, &ControlMessage::RequestRelay {
                    node_id: config.node_id.clone(),
                    session_token: config.session_token.clone(),
                    target_id: peer.node_id.clone(),
                }).await?;
                match resp {
                    ControlMessage::RelayAssigned { relay_addr, target_id } => {
                        println!("   Using relay {} for {}", relay_addr, peer.name);
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

        let allowed_ip = format!("{}/32", peer.vpn_ip);
        if let Err(e) = wg.add_peer(&peer.public_key, &allowed_ip, Some(&peer_endpoint)) {
            println!("   Failed to add WG peer {}: {}", peer.name, e);
        }
    }
```

with:

```rust
    for peer in &peers {
        println!("   Requesting relay for {}...", peer.name);
        let resp = conn_rpc(&conn, &ControlMessage::RequestRelay {
            node_id: config.node_id.clone(),
            session_token: config.session_token.clone(),
            target_id: peer.node_id.clone(),
        }).await?;
        let relay_addr = match resp {
            ControlMessage::RelayAssigned { relay_addr, target_id } => {
                if let Some(tid) = target_id {
                    if let Err(e) = register_with_relay(&relay_addr, &config.node_id, &tid).await {
                        println!("   Warning: relay registration failed for {}: {}", peer.name, e);
                    }
                }
                relay_addr
            }
            _ => {
                println!("   Failed to get relay for {}, skipping", peer.name);
                continue;
            }
        };
        println!("   Using relay {} for {}", relay_addr, peer.name);
        let allowed_ip = format!("{}/32", peer.vpn_ip);
        if let Err(e) = wg.add_peer(&peer.public_key, &allowed_ip, Some(&relay_addr)) {
            println!("   Failed to add WG peer {}: {}", peer.name, e);
        }
    }
```

- [ ] **Step 3: Build the cli crate**

Run: `cargo build -p tinyvpn-cli`
Expected: Compiles. (`tinyvpn-p2p` is still a dependency but now unused — that's fine until Task 2. `register_with_relay` still uses `std::net::SocketAddr`, so no unused-import error.)

- [ ] **Step 4: Commit**

```bash
git add crates/cli/src/main.rs
git commit -m "refactor(cli): relay-only connect, remove STUN and hole-punching"
```

---

### Task 2: Drop `tinyvpn-p2p` dependency from the cli

**Files:**
- Modify: `crates/cli/Cargo.toml`

- [ ] **Step 1: Remove the dependency line**

Delete this line from `crates/cli/Cargo.toml`:

```toml
tinyvpn-p2p = { workspace = true }
```

(The file's `[dependencies]` block ends with `tinyvpn-core = { workspace = true }` followed by `tinyvpn-p2p = { workspace = true }`; remove only the `tinyvpn-p2p` line.)

- [ ] **Step 2: Build the cli crate**

Run: `cargo build -p tinyvpn-cli`
Expected: Compiles cleanly (cli no longer references `tinyvpn_p2p`).

- [ ] **Step 3: Commit**

```bash
git add crates/cli/Cargo.toml
git commit -m "chore(cli): drop unused tinyvpn-p2p dependency"
```

---

### Task 3: Delete the `p2p` crate from the workspace

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Delete: `crates/p2p/` (entire directory: `src/lib.rs`, `src/stun.rs`, `src/puncher.rs`, `Cargo.toml`)

- [ ] **Step 1: Remove `crates/p2p` from workspace members**

In root `Cargo.toml`, the `[workspace] members` list contains `"crates/p2p",`. Remove that one line so members becomes:

```toml
members = [
    "crates/core",
    "crates/ccs",
    "crates/relay",
    "crates/cli",
]
```

- [ ] **Step 2: Remove `tinyvpn-p2p` from workspace dependencies**

In root `Cargo.toml`, under `[workspace.dependencies]`, the `# Internal crates` section has:

```toml
tinyvpn-core = { path = "crates/core" }
tinyvpn-p2p = { path = "crates/p2p" }
```

Remove the `tinyvpn-p2p` line, leaving:

```toml
tinyvpn-core = { path = "crates/core" }
```

- [ ] **Step 3: Delete the crate directory**

Run: `git rm -r crates/p2p`
Expected: files staged for deletion.

- [ ] **Step 4: Build the whole workspace**

Run: `cargo build --workspace`
Expected: Compiles. (4 crates now: core, ccs, relay, cli.)

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml
git commit -m "chore: remove tinyvpn-p2p crate from workspace"
```

(The `git rm -r crates/p2p` from Step 3 is already staged.)

---

### Task 4: Remove the `STUN_SERVERS` constant from core config

**Files:**
- Modify: `crates/core/src/config.rs` (lines ~10–14)

- [ ] **Step 1: Delete the constant**

Remove this block from `crates/core/src/config.rs`:

```rust
/// STUN server used for NAT detection
pub const STUN_SERVERS: &[&str] = &[
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302",
];
```

(plus the blank line after it, to avoid a double blank line between `CCS_DEFAULT_ADDR` and the `NodeConfig` doc comment).

- [ ] **Step 2: Build + test the core crate**

Run: `cargo test -p tinyvpn-core`
Expected: 15 tests pass. No test referenced `STUN_SERVERS`, so nothing breaks.

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/config.rs
git commit -m "refactor(core): remove unused STUN_SERVERS constant"
```

---

### Task 5: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Full workspace test**

Run: `cargo test --workspace`
Expected: 33 tests pass — core 15, ccs 15, relay 3. (p2p contributed 0 tests, now gone.)

- [ ] **Step 2: Clippy**

Run: `cargo clippy --workspace`
Expected: only the pre-existing `warning: method get_peer is never used` in ccs (unchanged, test-only). No new warnings, no errors.

- [ ] **Step 3: Sanity-run the cli help (optional)**

Run: `cargo run -p tinyvpn-cli -- --help`
Expected: prints help showing `connect` subcommand still present.

- [ ] **Step 4: If anything was left uncommitted, commit it**

```bash
git status   # expect clean (or only unrelated changes)
```

---

## Self-Review

- **Spec coverage:** ✅ cli connect simplified (Task 1); p2p crate deleted (Tasks 2–3); `STUN_SERVERS` removed (Task 4); CCS untouched (intentional); tests verified (Task 5). Every spec section maps to a task.
- **Placeholder scan:** ✅ No TBD/TODO; every code step shows exact code or exact commands.
- **Type consistency:** ✅ `register_with_relay(&relay_addr, &config.node_id, &tid)` signature unchanged from existing code; `ControlMessage::RelayAssigned { relay_addr, target_id }` and `RequestRelay { target_id }` match the existing protocol (no protocol changes). `wg.add_peer(public_key, allowed_ip, Some(endpoint))` signature unchanged.
- **Ordering:** Task 1 removes all `tinyvpn_p2p` references before Task 2 drops the dep; Task 2 drops the dep before Task 3 deletes the crate; Task 4 removes `STUN_SERVERS` only after p2p (its sole consumer) is gone. Build stays green at each task boundary.
