# TinyVPN 产品部署说明

## 系统要求

- **操作系统：** Linux（内核 ≥ 5.6，支持 WireGuard 模块）
- **软件依赖：** `wireguard-tools`（提供 `wg`、`wg-quick` 命令）
- **权限：** root（WireGuard 接口管理需要特权）
- **网络：** 节点需要能访问 CCS 服务器和公网 STUN 服务器

## 安装

### 方式一：从源码编译

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 安装 WireGuard 工具
apt install wireguard-tools   # Debian/Ubuntu
yum install wireguard-tools   # CentOS/RHEL

# 编译 TinyVPN
git clone <repo-url> && cd tinyvpn
cargo build --release

# 二进制文件位于 target/release/
ls target/release/tinyvpn-ccs     # 控制服务器
ls target/release/tinyvpn-relay   # 中继服务器
ls target/release/tinyvpn-cli     # 客户端
```

### 方式二：预编译二进制

```bash
# 下载 release 包后解压
tar xzf tinyvpn-<version>-linux-amd64.tar.gz
chmod +x tinyvpn-ccs tinyvpn-relay tinyvpn-cli
```

## 部署架构

```
                    ┌──────────────┐
                    │   CCS 服务器  │ :9090/TCP
                    │  (协调服务)   │
                    └──────┬───────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
        ┌─────┴─────┐ ┌───┴─────┐ ┌───┴─────┐
        │  Node A   │ │ Node B  │ │ Node C  │
        │ 10.13.0.1 │ │10.13.0.2│ │10.13.0.3│
        └───────────┘ └─────────┘ └─────────┘
              │            │
              └─ P2P直连 ──┘  或通过 Relay :9091/UDP
```

**最少需要两台机器：**
- 1 台运行 CCS（可同时运行 Relay）
- 至少 1 台运行 CLI 客户端

## 服务端部署

### 1. 启动 CCS（控制协调服务器）

CCS 是核心组件，负责节点注册、密钥交换、拓扑管理。

```bash
# 前台运行（测试用）
CCS_ADDR=0.0.0.0:9090 RELAY_ADDR=your-relay-ip:9091 ./tinyvpn-ccs

# 后台运行（生产用）
nohup ./tinyvpn-ccs > /var/log/tinyvpn-ccs.log 2>&1 &
```

**环境变量：**

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CCS_ADDR` | `0.0.0.0:9090` | CCS 监听地址 |
| `RELAY_ADDR` | `127.0.0.1:9091` | Relay 服务器地址，告知客户端 |

### 2. 启动 Relay（中继服务器）

Relay 在打洞失败时转发节点间流量。可以和 CCS 部署在同一台机器上。

```bash
# 前台运行
RELAY_ADDR=0.0.0.0:9091 ./tinyvpn-relay

# 后台运行
nohup ./tinyvpn-relay > /var/log/tinyvpn-relay.log 2>&1 &
```

**环境变量：**

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `RELAY_ADDR` | `0.0.0.0:9091` | Relay UDP 监听地址 |

### 3. 防火墙配置

```bash
# CCS 服务器 — 开放 TCP 9090
iptables -A INPUT -p tcp --dport 9090 -j ACCEPT

# Relay 服务器 — 开放 UDP 9091
iptables -A INPUT -p udp --dport 9091 -j ACCEPT

# 所有节点 — 开放 WireGuard 端口（默认 51820/UDP）
iptables -A INPUT -p udp --dport 51820 -j ACCEPT
```

## 客户端使用

### 注册节点

首次使用需要注册（每台机器只需一次）：

```bash
./tinyvpn-cli --ccs <ccs-ip>:9090 register --name my-node
```

注册成功后配置保存在 `~/.tinyvpn/config.json`。

### 连接网络

```bash
./tinyvpn-cli --ccs <ccs-ip>:9090 connect
```

可指定接口名和端口（避免与其他 WireGuard 接口冲突）：

```bash
./tinyvpn-cli --ccs <ccs-ip>:9090 --interface wg1 --port 51821 connect
```

连接后该节点获得 VPN IP（如 `10.13.0.x`），可以 ping 其他节点的 VPN IP。

### 查看状态

```bash
./tinyvpn-cli --ccs <ccs-ip>:9090 status
```

输出示例：
```
Node: node-a (node-2c998e69c324f566)
   VPN IP: 10.13.0.1
   Peers: 2 online
   - node-b (10.13.0.2) → 47.251.143.2:52717 [online]
   - node-c (10.13.0.3) → unknown [offline]
```

### 断开连接

```bash
./tinyvpn-cli disconnect
# 如果用了自定义接口名：
./tinyvpn-cli --interface wg1 disconnect
```

## 完整部署示例

假设有两台机器通过公网互通：

**服务器（1.2.3.4）：**
```bash
# 启动 CCS + Relay
CCS_ADDR=0.0.0.0:9090 RELAY_ADDR=1.2.3.4:9091 ./tinyvpn-ccs &
RELAY_ADDR=0.0.0.0:9091 ./tinyvpn-relay &
```

**节点 A（任意机器）：**
```bash
./tinyvpn-cli --ccs 1.2.3.4:9090 register --name office
./tinyvpn-cli --ccs 1.2.3.4:9090 connect
# 输出: VPN IP 10.13.0.1
```

**节点 B（另一台机器）：**
```bash
./tinyvpn-cli --ccs 1.2.3.4:9090 register --name home
./tinyvpn-cli --ccs 1.2.3.4:9090 connect
# 输出: VPN IP 10.13.0.2
```

**验证连通性：**
```bash
# 在节点 A 上
ping 10.13.0.2
```

## 日志与调试

设置环境变量 `RUST_LOG` 控制日志级别：

```bash
RUST_LOG=tinyvpn=debug ./tinyvpn-cli --ccs 1.2.3.4:9090 connect
```

| 级别 | 用途 |
|------|------|
| `error` | 仅错误 |
| `warn` | 错误 + 警告（默认） |
| `info` | 一般信息，推荐日常使用 |
| `debug` | 详细调试信息 |

## 常见问题

**Q: `wg setconf` 报错 "Line unrecognized: Address"**
A: 已修复。确保使用最新版本的 tinyvpn-core。

**Q: "UDP port 51820 is already in use"**
A: 系统 WireGuard 接口占用了端口。使用 `--port 51821` 指定其他端口。

**Q: 所有 peer 显示 "no public endpoint yet"**
A: STUN 无法发现公网地址（防火墙或 NAT 限制）。节点将依赖 relay 中继。

**Q: ping 不通但接口已创建**
A: 检查 peer 的 endpoint 是否正确设置。同一台机器测试时打洞会失败，但同一主机可以通过本地路由直达。
