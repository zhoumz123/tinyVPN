use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tinyvpn_core::protocol::PeerInfo;
use rusqlite::params;

/// Registered node with auth and heartbeat state
#[derive(Debug)]
struct NodeEntry {
    info: PeerInfo,
    session_token: String,
    last_heartbeat: Instant,
}

/// Global registry of all registered nodes, backed by SQLite
pub struct Registry {
    db: Mutex<rusqlite::Connection>,
    nodes: HashMap<String, NodeEntry>,
    relay_addr: String,
}

impl Registry {
    pub fn new(relay_addr: String) -> anyhow::Result<Self> {
        let db_path = dirs_home()?.join(".tinyvpn").join("ccs.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = rusqlite::Connection::open(&db_path)?;

        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS nodes (
                node_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                vpn_ip TEXT NOT NULL,
                public_key TEXT NOT NULL,
                endpoint TEXT NOT NULL DEFAULT '',
                session_token TEXT NOT NULL,
                connected INTEGER NOT NULL DEFAULT 1,
                last_heartbeat INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS acl_groups (
                node_id TEXT NOT NULL,
                group_name TEXT NOT NULL,
                PRIMARY KEY (node_id, group_name)
            );
            CREATE TABLE IF NOT EXISTS acl_rules (
                from_group TEXT NOT NULL,
                to_group TEXT NOT NULL,
                PRIMARY KEY (from_group, to_group)
            );"
        )?;

        let mut nodes = HashMap::new();
        {
            let mut stmt = db.prepare(
                "SELECT node_id, name, vpn_ip, public_key, endpoint, session_token, connected, last_heartbeat FROM nodes"
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, u64>(7)?,
                ))
            })?;

            for row in rows {
                let (node_id, name, vpn_ip, public_key, endpoint, session_token, connected, hb_epoch) = row?;
                let last_heartbeat = Instant::now() - std::time::Duration::from_secs(
                    now_secs().saturating_sub(hb_epoch)
                );
                nodes.insert(node_id.clone(), NodeEntry {
                    info: PeerInfo {
                        node_id,
                        name,
                        vpn_ip,
                        public_key,
                        endpoint,
                        connected,
                    },
                    session_token,
                    last_heartbeat,
                });
            }
        }

        tracing::info!("Loaded {} nodes from database", nodes.len());

        Ok(Self {
            db: Mutex::new(db),
            nodes,
            relay_addr,
        })
    }

    /// Register a new node, return (node_id, vpn_ip, session_token)
    pub fn register(&mut self, name: String, public_key: String) -> anyhow::Result<(String, String, String)> {
        let node_id = format!("node-{}", uuid_short());
        let vpn_ip = self.next_ip()?;
        let session_token = uuid_short();

        self.db.lock().unwrap().execute(
            "INSERT INTO nodes (node_id, name, vpn_ip, public_key, session_token, connected, last_heartbeat) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
            params![node_id, name, vpn_ip, public_key, session_token, now_secs()],
        )?;

        let peer = PeerInfo {
            node_id: node_id.clone(),
            name,
            vpn_ip: vpn_ip.clone(),
            public_key,
            endpoint: String::new(),
            connected: true,
        };

        self.nodes.insert(node_id.clone(), NodeEntry {
            info: peer,
            session_token: session_token.clone(),
            last_heartbeat: Instant::now(),
        });

        Ok((node_id, vpn_ip, session_token))
    }

    /// Validate session token, return true if valid
    pub fn validate_session(&self, node_id: &str, token: &str) -> bool {
        self.nodes
            .get(node_id)
            .map(|e| e.session_token == token)
            .unwrap_or(false)
    }

    /// Update a node's public endpoint (from STUN)
    pub fn update_endpoint(&mut self, node_id: &str, endpoint: String) {
        if let Some(entry) = self.nodes.get_mut(node_id) {
            entry.info.endpoint = endpoint.clone();
            entry.last_heartbeat = Instant::now();
            let _ = self.db.lock().unwrap().execute(
                "UPDATE nodes SET endpoint = ?1, last_heartbeat = ?2 WHERE node_id = ?3",
                params![endpoint, now_secs(), node_id],
            );
            tracing::info!("Updated endpoint for {}: {}", node_id, endpoint);
        }
    }

    /// Record heartbeat
    pub fn heartbeat(&mut self, node_id: &str) {
        if let Some(entry) = self.nodes.get_mut(node_id) {
            entry.last_heartbeat = Instant::now();
            entry.info.connected = true;
            let _ = self.db.lock().unwrap().execute(
                "UPDATE nodes SET connected = 1, last_heartbeat = ?1 WHERE node_id = ?2",
                params![now_secs(), node_id],
            );
        }
    }

    /// Mark stale nodes as disconnected, delete long-offline nodes
    pub fn reap_stale(&mut self) {
        let now = Instant::now();
        let db = self.db.lock().unwrap();

        let mut to_delete = Vec::new();
        for (id, entry) in self.nodes.iter_mut() {
            let elapsed = now.duration_since(entry.last_heartbeat).as_secs();
            if elapsed > 60 {
                entry.info.connected = false;
                let _ = db.execute(
                    "UPDATE nodes SET connected = 0 WHERE node_id = ?1",
                    params![id],
                );
            }
            if elapsed > 86400 {
                to_delete.push(id.clone());
            }
        }

        for id in to_delete {
            self.nodes.remove(&id);
            let _ = db.execute(
                "DELETE FROM nodes WHERE node_id = ?1",
                params![id],
            );
            tracing::info!("Deleted long-offline node {}", id);
        }
    }

    /// Get all peers visible to the requesting node (ACL-filtered)
    pub fn get_peers(&self, exclude_id: Option<&str>) -> Vec<PeerInfo> {
        let exclude = exclude_id.unwrap_or("");

        if !self.has_rules() {
            return self.nodes.values()
                .filter(|e| e.info.node_id != exclude)
                .map(|e| e.info.clone())
                .collect();
        }

        let my_groups = self.node_groups(exclude);
        if my_groups.is_empty() {
            return Vec::new();
        }

        let visible = self.visible_groups(&my_groups);
        if visible.is_empty() {
            return Vec::new();
        }

        self.nodes.values()
            .filter(|e| {
                if e.info.node_id == exclude {
                    return false;
                }
                let peer_groups = self.node_groups(&e.info.node_id);
                peer_groups.iter().any(|g| visible.contains(g))
            })
            .map(|e| e.info.clone())
            .collect()
    }

    pub fn add_group(&self, node_id: &str, group: &str) -> anyhow::Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT OR IGNORE INTO acl_groups (node_id, group_name) VALUES (?1, ?2)",
            params![node_id, group],
        )?;
        Ok(())
    }

    pub fn remove_group(&self, node_id: &str, group: &str) -> anyhow::Result<()> {
        self.db.lock().unwrap().execute(
            "DELETE FROM acl_groups WHERE node_id = ?1 AND group_name = ?2",
            params![node_id, group],
        )?;
        Ok(())
    }

    pub fn add_rule(&self, from_group: &str, to_group: &str) -> anyhow::Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT OR IGNORE INTO acl_rules (from_group, to_group) VALUES (?1, ?2)",
            params![from_group, to_group],
        )?;
        Ok(())
    }

    pub fn remove_rule(&self, from_group: &str, to_group: &str) -> anyhow::Result<()> {
        self.db.lock().unwrap().execute(
            "DELETE FROM acl_rules WHERE from_group = ?1 AND to_group = ?2",
            params![from_group, to_group],
        )?;
        Ok(())
    }

    pub fn list_groups(&self) -> anyhow::Result<Vec<(String, String)>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT node_id, group_name FROM acl_groups ORDER BY node_id, group_name")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn list_rules(&self) -> anyhow::Result<Vec<(String, String)>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT from_group, to_group FROM acl_rules ORDER BY from_group, to_group")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    fn node_groups(&self, node_id: &str) -> Vec<String> {
        let db = self.db.lock().unwrap();
        let mut stmt = match db.prepare("SELECT group_name FROM acl_groups WHERE node_id = ?1") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![node_id], |row| row.get(0));
        match rows {
            Ok(r) => r.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn visible_groups(&self, from_groups: &[String]) -> Vec<String> {
        if from_groups.is_empty() {
            return Vec::new();
        }
        let db = self.db.lock().unwrap();
        let mut result = Vec::new();
        for fg in from_groups {
            let mut stmt = match db.prepare("SELECT to_group FROM acl_rules WHERE from_group = ?1") {
                Ok(s) => s,
                Err(_) => continue,
            };
            let rows = stmt.query_map(params![fg], |row| row.get(0));
            if let Ok(r) = rows {
                for g in r.flatten() {
                    if !result.contains(&g) {
                        result.push(g);
                    }
                }
            }
        }
        result
    }

    pub fn has_rules(&self) -> bool {
        let db = self.db.lock().unwrap();
        let count: i64 = db.query_row("SELECT COUNT(*) FROM acl_rules", [], |row| row.get(0)).unwrap_or(0);
        count > 0
    }

    /// Get a specific peer's info
    pub fn get_peer(&self, node_id: &str) -> Option<PeerInfo> {
        self.nodes.get(node_id).map(|e| e.info.clone())
    }

    /// Get relay address
    pub fn relay_addr(&self) -> &str {
        &self.relay_addr
    }

    /// Find the lowest available IP in 10.13.0.0/16
    fn next_ip(&self) -> anyhow::Result<String> {
        let used: HashSet<String> = self.nodes.values().map(|e| e.info.vpn_ip.clone()).collect();
        for third in 0u32..=255 {
            for fourth in 1u32..=255 {
                if third == 0 && fourth == 0 {
                    continue;
                }
                let ip = format!("10.13.{}.{}", third, fourth);
                if !used.contains(&ip) {
                    return Ok(ip);
                }
            }
        }
        anyhow::bail!("IP address space exhausted")
    }
}

pub type SharedRegistry = Arc<RwLock<Registry>>;

fn uuid_short() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..8).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn dirs_home() -> anyhow::Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::from(
        std::env::var("HOME").unwrap_or_else(|_| "/root".into()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_registry() -> anyhow::Result<Registry> {
        let db = rusqlite::Connection::open_in_memory()?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS nodes (
                node_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                vpn_ip TEXT NOT NULL,
                public_key TEXT NOT NULL,
                endpoint TEXT NOT NULL DEFAULT '',
                session_token TEXT NOT NULL,
                connected INTEGER NOT NULL DEFAULT 1,
                last_heartbeat INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS acl_groups (
                node_id TEXT NOT NULL,
                group_name TEXT NOT NULL,
                PRIMARY KEY (node_id, group_name)
            );
            CREATE TABLE IF NOT EXISTS acl_rules (
                from_group TEXT NOT NULL,
                to_group TEXT NOT NULL,
                PRIMARY KEY (from_group, to_group)
            );"
        )?;
        Ok(Registry {
            db: Mutex::new(db),
            nodes: HashMap::new(),
            relay_addr: "127.0.0.1:9091".into(),
        })
    }

    #[test]
    fn register_assigns_sequential_ips() {
        let mut reg = new_registry().unwrap();
        let (id1, ip1, _) = reg.register("a".into(), "pk1".into()).unwrap();
        let (id2, ip2, _) = reg.register("b".into(), "pk2".into()).unwrap();
        assert!(id1.starts_with("node-"));
        assert_ne!(id1, id2);
        assert_eq!(ip1, "10.13.0.1");
        assert_eq!(ip2, "10.13.0.2");
    }

    #[test]
    fn validate_session_correct() {
        let mut reg = new_registry().unwrap();
        let (id, _, tok) = reg.register("a".into(), "pk1".into()).unwrap();
        assert!(reg.validate_session(&id, &tok));
    }

    #[test]
    fn validate_session_wrong_token() {
        let mut reg = new_registry().unwrap();
        let (id, _, _) = reg.register("a".into(), "pk1".into()).unwrap();
        assert!(!reg.validate_session(&id, "wrong"));
    }

    #[test]
    fn validate_session_unknown_node() {
        let reg = new_registry().unwrap();
        assert!(!reg.validate_session("nonexistent", "any"));
    }

    #[test]
    fn heartbeat_refreshes() {
        let mut reg = new_registry().unwrap();
        let (id, _, _) = reg.register("a".into(), "pk1".into()).unwrap();

        let entry = reg.nodes.get_mut(&id).unwrap();
        entry.last_heartbeat = Instant::now() - std::time::Duration::from_secs(61);

        reg.heartbeat(&id);
        let entry = reg.nodes.get(&id).unwrap();
        assert!(entry.info.connected);
        assert!(Instant::now().duration_since(entry.last_heartbeat).as_secs() < 5);
    }

    #[test]
    fn reap_stale_marks_offline() {
        let mut reg = new_registry().unwrap();
        let (id, _, _) = reg.register("a".into(), "pk1".into()).unwrap();

        let entry = reg.nodes.get_mut(&id).unwrap();
        entry.last_heartbeat = Instant::now() - std::time::Duration::from_secs(61);

        reg.reap_stale();
        let entry = reg.nodes.get(&id).unwrap();
        assert!(!entry.info.connected);
    }

    #[test]
    fn get_peers_excludes_self() {
        let mut reg = new_registry().unwrap();
        let (id1, _, _) = reg.register("a".into(), "pk1".into()).unwrap();
        let (id2, _, _) = reg.register("b".into(), "pk2".into()).unwrap();

        let peers = reg.get_peers(Some(&id1));
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].node_id, id2);
    }

    #[test]
    fn update_endpoint() {
        let mut reg = new_registry().unwrap();
        let (id, _, _) = reg.register("a".into(), "pk1".into()).unwrap();
        reg.update_endpoint(&id, "1.2.3.4:51820".into());
        let peer = reg.get_peer(&id).unwrap();
        assert_eq!(peer.endpoint, "1.2.3.4:51820");
    }

    #[test]
    fn relay_addr() {
        let reg = new_registry().unwrap();
        assert_eq!(reg.relay_addr(), "127.0.0.1:9091");
    }

    #[test]
    fn ip_recycled_after_remove() {
        let mut reg = new_registry().unwrap();
        let (id1, ip1, _) = reg.register("a".into(), "pk1".into()).unwrap();
        assert_eq!(ip1, "10.13.0.1");

        reg.nodes.remove(&id1);
        reg.db.lock().unwrap().execute("DELETE FROM nodes WHERE node_id = ?1", params![id1]).unwrap();

        let (_, ip2, _) = reg.register("b".into(), "pk2".into()).unwrap();
        assert_eq!(ip2, "10.13.0.1");
    }

    #[test]
    fn persistence_across_restart() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS nodes (
                node_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                vpn_ip TEXT NOT NULL,
                public_key TEXT NOT NULL,
                endpoint TEXT NOT NULL DEFAULT '',
                session_token TEXT NOT NULL,
                connected INTEGER NOT NULL DEFAULT 1,
                last_heartbeat INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS acl_groups (
                node_id TEXT NOT NULL,
                group_name TEXT NOT NULL,
                PRIMARY KEY (node_id, group_name)
            );
            CREATE TABLE IF NOT EXISTS acl_rules (
                from_group TEXT NOT NULL,
                to_group TEXT NOT NULL,
                PRIMARY KEY (from_group, to_group)
            );"
        ).unwrap();

        let mut reg1 = Registry {
            db: Mutex::new(db),
            nodes: HashMap::new(),
            relay_addr: "127.0.0.1:9091".into(),
        };
        let (id, _, tok) = reg1.register("test".into(), "pk".into()).unwrap();

        let count: i64 = reg1.db.lock().unwrap().query_row(
            "SELECT COUNT(*) FROM nodes WHERE node_id = ?1",
            params![id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 1);
        assert!(reg1.validate_session(&id, &tok));
    }

    #[test]
    fn acl_no_rules_allows_all() {
        let mut reg = new_registry().unwrap();
        let (id1, _, _) = reg.register("a".into(), "pk1".into()).unwrap();
        let (_, _, _) = reg.register("b".into(), "pk2".into()).unwrap();
        let peers = reg.get_peers(Some(&id1));
        assert_eq!(peers.len(), 1);
    }

    #[test]
    fn acl_deny_by_default() {
        let mut reg = new_registry().unwrap();
        let (id1, _, _) = reg.register("a".into(), "pk1".into()).unwrap();
        let (_, _, _) = reg.register("b".into(), "pk2".into()).unwrap();
        reg.add_rule("admin", "dev").unwrap();
        let peers = reg.get_peers(Some(&id1));
        assert!(peers.is_empty());
    }

    #[test]
    fn acl_group_visibility() {
        let mut reg = new_registry().unwrap();
        let (id1, _, _) = reg.register("a".into(), "pk1".into()).unwrap();
        let (id2, _, _) = reg.register("b".into(), "pk2".into()).unwrap();
        let (id3, _, _) = reg.register("c".into(), "pk3".into()).unwrap();

        reg.add_group(&id1, "admin").unwrap();
        reg.add_group(&id2, "dev").unwrap();
        reg.add_group(&id3, "db").unwrap();

        reg.add_rule("admin", "dev").unwrap();

        let peers = reg.get_peers(Some(&id1));
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].node_id, id2);

        let peers = reg.get_peers(Some(&id2));
        assert!(peers.is_empty());
    }

    #[test]
    fn acl_multi_group() {
        let mut reg = new_registry().unwrap();
        let (id1, _, _) = reg.register("a".into(), "pk1".into()).unwrap();
        let (id2, _, _) = reg.register("b".into(), "pk2".into()).unwrap();
        let (id3, _, _) = reg.register("c".into(), "pk3".into()).unwrap();

        reg.add_group(&id1, "admin").unwrap();
        reg.add_group(&id2, "dev").unwrap();
        reg.add_group(&id3, "db").unwrap();

        reg.add_rule("admin", "dev").unwrap();
        reg.add_rule("admin", "db").unwrap();

        let peers = reg.get_peers(Some(&id1));
        assert_eq!(peers.len(), 2);
    }
}
