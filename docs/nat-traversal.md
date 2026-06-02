# 内网穿透教程

通过 TinyVPN 从公网服务器访问公司内网服务器。

## 场景

- 公网服务器 IP：`1.2.3.4`（云主机，有公网 IP）
- 公司内网服务器（在 NAT 后面，无法被外部直接访问）
- 目标：从公网服务器 SSH 和访问内网服务器上的 Web 服务

## 部署步骤

### 1. 公网服务器上启动 CCS 和 Relay

```bash
# CCS（控制服务器）+ Relay（中继）
CCS_ADDR=0.0.0.0:9090 RELAY_ADDR=1.2.3.4:9091 ./tinyvpn-ccs &
RELAY_ADDR=0.0.0.0:9091 ./tinyvpn-relay &
```

### 2. 公司内网服务器注册并连接

```bash
# 注册
./tinyvpn-cli --ccs 1.2.3.4:9090 register --name office-server
# 连接（会自动打洞或走 relay）
./tinyvpn-cli --ccs 1.2.3.4:9090 connect
# 假设分配到 VPN IP: 10.13.0.1
```

### 3. 公网服务器也注册并连接

```bash
./tinyvpn-cli --ccs 127.0.0.1:9090 register --name public-server
./tinyvpn-cli --ccs 127.0.0.1:9090 connect
# 假设分配到 VPN IP: 10.13.0.2
```

### 4. 在公网服务器上转发端口

假设内网服务器跑了 SSH (22) 和 Web (80)：

```bash
# 转发 SSH
./tinyvpn-cli forward --vpn-ip 10.13.0.1 --remote-port 22 --local-port 2222

# 转发 Web
./tinyvpn-cli forward --vpn-ip 10.13.0.1 --remote-port 80 --local-port 8080
```

### 5. 访问内网服务

```bash
# SSH 到内网服务器
ssh -p 2222 user@127.0.0.1

# 访问内网 Web
curl http://127.0.0.1:8080
```

## 网络拓扑

```
  公网服务器 (1.2.3.4)
  ├── CCS :9090 (控制)
  ├── Relay :9091 (中继)
  ├── wg0: 10.13.0.2
  │
  │  ← forward :2222 → 10.13.0.1:22
  │  ← forward :8080 → 10.13.0.1:80
  │
  │         VPN 隧道 (WireGuard)
  │
  公司内网服务器 (NAT 后面)
  ├── wg0: 10.13.0.1
  ├── SSH :22
  └── Web :80
```

## 注意事项

- 公司内网服务器需要能访问公网的 `1.2.3.4:9090`（QUIC）和 `1.2.3.4:9091`（UDP）
- 内网服务器上执行 `connect` 需要 root 权限（创建 WireGuard 接口）
- 如果两边都在 NAT 后面且打洞失败，流量会自动走 Relay 中继，性能略低但仍可用
