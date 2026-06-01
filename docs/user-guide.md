# TinyVPN 产品使用说明书

**版本：** 0.1.0
**更新日期：** 2026-06-01

---

## 1. 产品简介

TinyVPN 是一款轻量级 Mesh VPN 组网工具，基于 Rust 开发，使用 WireGuard 隧道协议。它可以将分布在不同网络中的设备组成一个虚拟局域网，设备间通过加密隧道直接通信。

### 核心特性

- **Mesh 组网** — 所有节点两两直连，无需中心化路由
- **NAT 穿透** — 基于 STUN 的 UDP 打洞，穿透大多数 NAT 类型
- **中继回退** — 打洞失败时自动通过 Relay 转发流量
- **WireGuard 加密** — 使用 X25519 密钥交换 + ChaCha20-Poly1305 加密
- **简单部署** — 一个控制服务器 + 一个中继服务器 + 客户端 CLI

### 适用场景

- 远程办公：在家访问公司内网
- 多地互联：多个办公室/机房网络互通
- 设备管理：远程管理分布式的服务器或 IoT 设备
- 开发测试：快速搭建跨网络的开发环境

---

## 2. 快速上手

### 2.1 安装前检查

```bash
# 检查 WireGuard 内核模块
lsmod | grep wireguard

# 检查 wg 命令
which wg

# 如果缺少，安装 wireguard-tools
apt install wireguard-tools    # Debian/Ubuntu
yum install wireguard-tools    # CentOS/RHEL
```

### 2.2 解压安装包

```bash
tar xzf tinyvpn-0.1.0-linux-aarch64.tar.gz
cd tinyvpn-0.1.0-linux-aarch64
```

包内结构：
```
tinyvpn-0.1.0-linux-aarch64/
├── bin/
│   ├── tinyvpn-ccs       # 控制协调服务器
│   ├── tinyvpn-relay     # 中继服务器
│   └── tinyvpn-cli       # 客户端命令行工具
├── scripts/
│   ├── start-ccs.sh      # 一键启动服务端
│   └── stop-all.sh       # 一键停止所有服务
└── docs/
    ├── user-guide.md     # 本文档
    ├── deployment.md     # 部署说明
    └── README.md         # 项目简介
```

### 2.3 五分钟组网

**第一步：在服务器上启动服务端**

```bash
cd tinyvpn-0.1.0-linux-aarch64
./scripts/start-ccs.sh
```

输出：
```
=== TinyVPN Server Starting ===
CCS:   0.0.0.0:9090 (TCP)
Relay: 0.0.0.0:9091 (UDP)
...
=== Started ===
```

**第二步：在机器 A 上注册并连接**

```bash
./bin/tinyvpn-cli --ccs <服务器IP>:9090 register --name office
./bin/tinyvpn-cli --ccs <服务器IP>:9090 connect
```

输出：
```
Connecting as office (10.13.0.1)...
...
WireGuard interface wg-tinyvpn is up (10.13.0.1)
TinyVPN is running. Press Ctrl+C to stop.
```

**第三步：在机器 B 上注册并连接**

```bash
./bin/tinyvpn-cli --ccs <服务器IP>:9090 register --name home
./bin/tinyvpn-cli --ccs <服务器IP>:9090 connect
```

输出：
```
Connecting as home (10.13.0.2)...
...
WireGuard interface wg-tinyvpn is up (10.13.0.2)
TinyVPN is running. Press Ctrl+C to stop.
```

**第四步：验证连通**

```bash
# 在机器 A 上
ping 10.13.0.2

# 在机器 B 上
ping 10.13.0.1
```

ping 通即组网成功！

---

## 3. 客户端命令详解

### 3.1 注册节点 — `register`

```bash
./bin/tinyvpn-cli --ccs <服务器IP>:9090 register --name <节点名>
```

| 参数 | 说明 |
|------|------|
| `--ccs` | CCS 服务器地址（IP:端口） |
| `--name` | 节点名称，便于识别，如 office、home、server-1 |

注册成功后：
- 生成 WireGuard 密钥对
- 从 CCS 获取唯一节点 ID 和 VPN IP（10.13.0.x）
- 配置保存在 `~/.tinyvpn/config.json`

**注意：** 每台机器只需注册一次。重复注册会提示已存在。

### 3.2 连接网络 — `connect`

```bash
./bin/tinyvpn-cli --ccs <服务器IP>:9090 connect
```

完整参数：
```bash
./bin/tinyvpn-cli --ccs <服务器IP>:9090 --interface wg-tinyvpn --port 51820 connect
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--ccs` | `127.0.0.1:9090` | CCS 服务器地址 |
| `--interface` | `wg-tinyvpn` | WireGuard 接口名 |
| `--port` | `51820` | WireGuard 监听端口（UDP） |

连接流程：
1. 建立 TCP 长连接到 CCS
2. 通过 STUN 发现公网端点
3. 上报端点到 CCS
4. 获取在线节点列表
5. 创建 WireGuard 虚拟网卡
6. 对每个在线节点：尝试 UDP 打洞 → 成功则直连，失败则走 Relay 中继
7. 保持心跳，按 Ctrl+C 断开

**如果端口 51820 被占用**（系统已有 WireGuard 接口），使用 `--port 51821` 指定其他端口。

### 3.3 查看状态 — `status`

```bash
./bin/tinyvpn-cli --ccs <服务器IP>:9090 status
```

输出示例：
```
Node: office (node-2c998e69c324f566)
   VPN IP: 10.13.0.1
   Peers: 2 online
   - home (10.13.0.2) → 203.0.113.5:52717 [online]
   - lab (10.13.0.3) → unknown [offline]
```

### 3.4 断开连接 — `disconnect`

```bash
./bin/tinyvpn-cli disconnect
```

拆除 WireGuard 接口，节点离线。如使用了自定义接口名：

```bash
./bin/tinyvpn-cli --interface wg1 disconnect
```

---

## 4. 服务端管理

### 4.1 环境变量

**CCS（控制协调服务器）：**

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CCS_ADDR` | `0.0.0.0:9090` | TCP 监听地址 |
| `RELAY_ADDR` | `127.0.0.1:9091` | 告知客户端的 Relay 地址 |

**Relay（中继服务器）：**

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `RELAY_ADDR` | `0.0.0.0:9091` | UDP 监听地址 |

### 4.2 手动启动

```bash
# 启动 Relay
RELAY_ADDR=0.0.0.0:9091 nohup ./bin/tinyvpn-relay > /var/log/tinyvpn-relay.log 2>&1 &

# 启动 CCS（RELAY_ADDR 设为服务器的公网 IP）
CCS_ADDR=0.0.0.0:9090 RELAY_ADDR=1.2.3.4:9091 nohup ./bin/tinyvpn-ccs > /var/log/tinyvpn-ccs.log 2>&1 &
```

### 4.3 停止服务

```bash
./scripts/stop-all.sh
# 或手动
pkill -f tinyvpn-ccs
pkill -f tinyvpn-relay
```

---

## 5. 网络规划

### VPN 地址池

默认使用 `10.13.0.0/16`，支持最多 65534 个节点。节点 IP 按 `10.13.0.1`、`10.13.0.2`、... 顺序分配。

### 端口清单

| 服务 | 协议 | 默认端口 | 用途 |
|------|------|----------|------|
| CCS | TCP | 9090 | 控制协议通信 |
| Relay | UDP | 9091 | 中继流量转发 |
| WireGuard | UDP | 51820 | VPN 隧道数据 |

### 防火墙规则

```bash
# 服务器端
iptables -A INPUT -p tcp --dport 9090 -j ACCEPT   # CCS
iptables -A INPUT -p udp --dport 9091 -j ACCEPT   # Relay

# 所有客户端节点
iptables -A INPUT -p udp --dport 51820 -j ACCEPT  # WireGuard
```

---

## 6. 日志与排错

### 查看日志

```bash
# 服务端日志
tail -f /var/log/tinyvpn-ccs.log
tail -f /var/log/tinyvpn-relay.log

# 客户端调试模式
RUST_LOG=tinyvpn=debug ./bin/tinyvpn-cli --ccs <ip>:9090 connect
```

### 日志级别

| 级别 | 命令 | 适用场景 |
|------|------|----------|
| error | `RUST_LOG=tinyvpn=error` | 仅查看错误 |
| warn | `RUST_LOG=tinyvpn=warn` | 日常使用 |
| info | `RUST_LOG=tinyvpn=info` | 查看连接流程 |
| debug | `RUST_LOG=tinyvpn=debug` | 问题排查 |

### 常见问题

| 现象 | 原因 | 解决方案 |
|------|------|----------|
| `UDP port 51820 is already in use` | 系统 WireGuard 占用端口 | 加 `--port 51821` |
| 所有 peer 显示 "no public endpoint yet" | STUN 失败 | 检查 UDP 出站是否被防火墙拦截 |
| `Not registered yet` | 未执行 register | 先运行 `register --name <name>` |
| ping 不通 | peer endpoint 未设置 | 确认两端都已 connect 并上报了端点 |
| `CCS disconnected` | CCS 未运行或地址错误 | 检查 CCS 进程和 `--ccs` 参数 |

---

## 7. 架构概览

```
                    ┌──────────────┐
                    │   CCS 服务器  │ :9090/TCP
                    │  节点注册     │
                    │  密钥交换     │
                    │  拓扑管理     │
                    └──────┬───────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
        ┌─────┴─────┐ ┌───┴─────┐ ┌───┴─────┐
        │  Node A   │ │ Node B  │ │ Node C  │
        │ 10.13.0.1 │ │10.13.0.2│ │10.13.0.3│
        │ WireGuard │ │WireGuard│ │WireGuard│
        └─────┬─────┘ └────┬────┘ └─────────┘
              │             │
              └── UDP 打洞 ─┘  ← 优先直连
                    │
              ┌─────┴──────┐
              │   Relay    │ :9091/UDP  ← 打洞失败时中继
              └────────────┘
```

**通信流程：**
1. 节点注册 → CCS 分配 VPN IP 和 session token
2. 节点 connect → STUN 发现公网 IP → 上报 CCS
3. 获取 peer 列表 → 逐个尝试 UDP 打洞
4. 打洞成功 → WireGuard 直连（加密隧道）
5. 打洞失败 → 通过 Relay 中继转发
6. 定期心跳保持在线状态（60 秒超时离线）

---

## 8. 安全说明

- 所有控制平面通信基于 session token 认证
- VPN 数据通过 WireGuard 内核模块加密（ChaCha20-Poly1305）
- 密钥交换使用 X25519 椭圆曲线算法
- 私钥仅存储在本地 `~/.tinyvpn/config.json`，不会传输到服务器
- MVP 阶段控制平面为明文 TCP+JSON，生产环境建议升级到 TLS 或 QUIC

---

## 9. 技术支持

- 日志级别设为 `debug` 后复现问题，将日志发送给开发团队
- 附带信息：操作系统版本、内核版本、网络环境（NAT 类型）
