use std::sync::Arc;
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::Html,
    routing::{get, post, delete},
    Json,
};
use tower_http::cors::CorsLayer;
use tokio::sync::RwLock;
use crate::registry::Registry;

pub type WebState = Arc<RwLock<Registry>>;

pub async fn run(addr: &str, registry: WebState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(dashboard))
        .route("/api/nodes", get(list_nodes))
        .route("/api/acl", get(list_acl))
        .route("/api/acl/group", post(add_group))
        .route("/api/acl/group", delete(remove_group))
        .route("/api/acl/rule", post(add_rule))
        .route("/api/acl/rule", delete(remove_rule))
        .layer(CorsLayer::permissive())
        .with_state(registry);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Web dashboard on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn list_nodes(State(reg): State<WebState>) -> Json<serde_json::Value> {
    let reg = reg.read().await;
    let peers = reg.get_peers(None);
    Json(serde_json::json!({ "nodes": peers }))
}

async fn list_acl(State(reg): State<WebState>) -> Json<serde_json::Value> {
    let reg = reg.read().await;
    let groups = reg.list_groups().unwrap_or_default();
    let rules = reg.list_rules().unwrap_or_default();
    Json(serde_json::json!({
        "groups": groups.into_iter().map(|(n, g)| serde_json::json!({"node_id": n, "group_name": g})).collect::<Vec<_>>(),
        "rules": rules.into_iter().map(|(f, t)| serde_json::json!({"from_group": f, "to_group": t})).collect::<Vec<_>>()
    }))
}

#[derive(serde::Deserialize)]
struct GroupReq {
    node_id: String,
    group_name: String,
}

async fn add_group(
    State(reg): State<WebState>,
    Json(req): Json<GroupReq>,
) -> StatusCode {
    let reg = reg.read().await;
    match reg.add_group(&req.node_id, &req.group_name) {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn remove_group(
    State(reg): State<WebState>,
    Json(req): Json<GroupReq>,
) -> StatusCode {
    let reg = reg.read().await;
    match reg.remove_group(&req.node_id, &req.group_name) {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(serde::Deserialize)]
struct RuleReq {
    from_group: String,
    to_group: String,
}

async fn add_rule(
    State(reg): State<WebState>,
    Json(req): Json<RuleReq>,
) -> StatusCode {
    let reg = reg.read().await;
    match reg.add_rule(&req.from_group, &req.to_group) {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn remove_rule(
    State(reg): State<WebState>,
    Json(req): Json<RuleReq>,
) -> StatusCode {
    let reg = reg.read().await;
    match reg.remove_rule(&req.from_group, &req.to_group) {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

const DASHBOARD_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>TinyVPN Dashboard</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#0f172a;color:#e2e8f0;min-height:100vh}
.header{background:#1e293b;padding:16px 24px;border-bottom:1px solid #334155;display:flex;align-items:center;gap:12px}
.header h1{font-size:20px;font-weight:600;color:#f8fafc}
.header .dot{width:8px;height:8px;border-radius:50%;background:#22c55e}
.container{max-width:1100px;margin:24px auto;padding:0 24px}
.tabs{display:flex;gap:4px;margin-bottom:20px;background:#1e293b;border-radius:8px;padding:4px}
.tab{padding:8px 16px;border-radius:6px;cursor:pointer;font-size:14px;font-weight:500;color:#94a3b8;border:none;background:none}
.tab.active{background:#334155;color:#f8fafc}
.tab:hover{color:#e2e8f0}
.panel{display:none}
.panel.active{display:block}
.card{background:#1e293b;border:1px solid #334155;border-radius:8px;padding:20px;margin-bottom:16px}
.card h2{font-size:16px;margin-bottom:16px;color:#f8fafc}
table{width:100%;border-collapse:collapse;font-size:14px}
th{text-align:left;padding:8px 12px;color:#94a3b8;font-weight:500;border-bottom:1px solid #334155}
td{padding:8px 12px;border-bottom:1px solid #1e293b}
.badge{display:inline-block;padding:2px 8px;border-radius:9999px;font-size:12px;font-weight:500}
.badge.on{background:#064e3b;color:#6ee7b7}
.badge.off{background:#450a0a;color:#fca5a5}
.badge.group{background:#1e1b4b;color:#a5b4fc;margin:1px}
.btn{padding:6px 14px;border-radius:6px;border:1px solid #334155;background:#334155;color:#e2e8f0;cursor:pointer;font-size:13px}
.btn:hover{background:#475569}
.btn.danger{border-color:#7f1d1d;background:#7f1d1d;color:#fecaca}
.btn.danger:hover{background:#991b1b}
.form-row{display:flex;gap:8px;margin-bottom:12px;align-items:center;flex-wrap:wrap}
input,select{padding:6px 10px;border-radius:6px;border:1px solid #334155;background:#0f172a;color:#e2e8f0;font-size:13px}
input:focus,select:focus{outline:none;border-color:#3b82f6}
.arrow{color:#64748b}
.stats{display:flex;gap:16px;margin-bottom:20px}
.stat{background:#1e293b;border:1px solid #334155;border-radius:8px;padding:16px 20px;flex:1}
.stat .label{font-size:12px;color:#94a3b8;margin-bottom:4px}
.stat .value{font-size:24px;font-weight:600;color:#f8fafc}
.empty{text-align:center;padding:32px;color:#64748b}
</style>
</head>
<body>
<div class="header">
<div class="dot"></div>
<h1>TinyVPN Dashboard</h1>
</div>
<div class="container">
<div class="stats" id="stats"></div>
<div class="tabs">
<button class="tab active" onclick="switchTab('nodes')">Nodes</button>
<button class="tab" onclick="switchTab('acl')">ACL</button>
</div>
<div id="nodes" class="panel active">
<div class="card"><h2>Registered Nodes</h2><table id="node-table"><thead><tr><th>Name</th><th>VPN IP</th><th>Node ID</th><th>Endpoint</th><th>Status</th><th>Groups</th></tr></thead><tbody></tbody></table></div>
</div>
<div id="acl" class="panel">
<div class="card">
<h2>Group Assignments</h2>
<div class="form-row">
<input id="g-node" placeholder="Node ID" style="width:220px">
<input id="g-name" placeholder="Group name" style="width:140px">
<button class="btn" onclick="addGroup()">Add</button>
<button class="btn danger" onclick="removeGroup()">Remove</button>
</div>
<table id="group-table"><thead><tr><th>Node ID</th><th>Group</th></tr></thead><tbody></tbody></table>
</div>
<div class="card">
<h2>ACL Rules</h2>
<div class="form-row">
<input id="r-from" placeholder="From group" style="width:140px">
<span class="arrow">&rarr;</span>
<input id="r-to" placeholder="To group" style="width:140px">
<button class="btn" onclick="addRule()">Allow</button>
<button class="btn danger" onclick="removeRule()">Remove</button>
</div>
<table id="rule-table"><thead><tr><th>From</th><th>To</th><th>Action</th></tr></thead><tbody></tbody></table>
</div>
</div>
</div>
<script>
function switchTab(id){
document.querySelectorAll('.tab').forEach(t=>t.classList.remove('active'));
document.querySelectorAll('.panel').forEach(p=>p.classList.remove('active'));
event.target.classList.add('active');
document.getElementById(id).classList.add('active');
}
async function api(path,opts){
const r=await fetch(path,opts);
return r.ok?r.json():null;
}
async function refresh(){
const[data,acl]=await Promise.all([api('/api/nodes'),api('/api/acl')]);
if(!data)return;
const nodes=data.nodes||[];
const online=nodes.filter(n=>n.connected).length;
document.getElementById('stats').innerHTML=`
<div class="stat"><div class="label">Total Nodes</div><div class="value">${nodes.length}</div></div>
<div class="stat"><div class="label">Online</div><div class="value">${online}</div></div>
<div class="stat"><div class="label">ACL Rules</div><div class="value">${(acl&&acl.rules)?acl.rules.length:0}</div></div>`;

const gMap={};
if(acl&&acl.groups)acl.groups.forEach(g=>{(gMap[g.node_id]=gMap[g.node_id]||[]);gMap[g.node_id].push(g.group_name)});

document.querySelector('#node-table tbody').innerHTML=nodes.length?nodes.map(n=>`<tr>
<td>${n.name}</td><td><code>${n.vpn_ip}</code></td><td style="font-size:12px;color:#94a3b8">${n.node_id}</td>
<td>${n.endpoint||'-'}</td>
<td><span class="badge ${n.connected?'on':'off'}">${n.connected?'online':'offline'}</span></td>
<td>${(gMap[n.node_id]||[]).map(g=>`<span class="badge group">${g}</span>`).join('')}</td>
</tr>`).join(''):'<tr><td colspan="6" class="empty">No nodes registered</td></tr>';

if(acl){
document.querySelector('#group-table tbody').innerHTML=acl.groups.length?acl.groups.map(g=>`<tr><td style="font-size:12px">${g.node_id}</td><td><span class="badge group">${g.group_name}</span></td></tr>`).join(''):'<tr><td colspan="2" class="empty">No groups defined</td></tr>';
document.querySelector('#rule-table tbody').innerHTML=acl.rules.length?acl.rules.map(r=>`<tr><td><span class="badge group">${r.from_group}</span></td><td><span class="badge group">${r.to_group}</span></td><td><span class="badge on">allow</span></td></tr>`).join(''):'<tr><td colspan="3" class="empty">No rules (all traffic allowed)</td></tr>';
}
}
async function addGroup(){await fetch('/api/acl/group',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({node_id:document.getElementById('g-node').value,group_name:document.getElementById('g-name').value})});refresh()}
async function removeGroup(){await fetch('/api/acl/group',{method:'DELETE',headers:{'Content-Type':'application/json'},body:JSON.stringify({node_id:document.getElementById('g-node').value,group_name:document.getElementById('g-name').value})});refresh()}
async function addRule(){await fetch('/api/acl/rule',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({from_group:document.getElementById('r-from').value,to_group:document.getElementById('r-to').value})});refresh()}
async function removeRule(){await fetch('/api/acl/rule',{method:'DELETE',headers:{'Content-Type':'application/json'},body:JSON.stringify({from_group:document.getElementById('r-from').value,to_group:document.getElementById('r-to').value})});refresh()}
refresh();setInterval(refresh,5000);
</script>
</body>
</html>"##;
