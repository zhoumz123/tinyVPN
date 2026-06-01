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

pub struct Relay {
    socket: Arc<UdpSocket>,
    sessions: Arc<Mutex<HashMap<SocketAddr, Session>>>,
}

impl Relay {
    pub async fn bind(addr: &str) -> Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        tracing::info!("Relay listening on {}", socket.local_addr()?);
        Ok(Self {
            socket: Arc::new(socket),
            sessions: Arc::new(Mutex::new(HashMap::new())),
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

        // Forwarding loop
        let mut buf = vec![0u8; BUF_SIZE];
        loop {
            let (n, from) = self.socket.recv_from(&mut buf).await?;
            let sessions = self.sessions.lock().await;

            if let Some(_session) = sessions.get(&from) {
                drop(sessions);
                // Update activity timestamp
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
}
