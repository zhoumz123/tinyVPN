# QUIC Transport Upgrade — Design Spec

Date: 2026-06-01

## Goal

Replace the TCP transport layer with QUIC while keeping newline-delimited JSON serialization unchanged. All TLS complexity hidden behind two simple functions.

## Approach

- One QUIC connection per client-to-CCS session
- One bidirectional stream per connection (same pattern as current TCP)
- Self-signed cert via rcgen, client skips verification
- Certificate logic isolated in `core/src/tls.rs`

## Changes

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | rustls add `dangerous_configuration` feature |
| `crates/core/Cargo.toml` | Add quinn, rustls, rcgen deps |
| `crates/core/src/tls.rs` | NEW: `create_server()` + `create_client()` |
| `crates/core/src/lib.rs` | Export `tls` module |
| `crates/ccs/src/server.rs` | TCP → QUIC: endpoint, accept_bi, SendStream/RecvStream |
| `crates/cli/Cargo.toml` | Add quinn, rustls deps |
| `crates/cli/src/main.rs` | TCP → QUIC: connect, open_bi, SendStream type |

## Scope

- Control plane only (CCS ↔ CLI)
- Relay and P2P stay UDP (unaffected)
- No protocol message changes
