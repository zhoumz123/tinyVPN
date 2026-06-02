# TinyVPN 产品使用说明书

**版本：** 0.1.0
**更新日期：** 2026-06-02

---

## 1. 产品简介

TinyVPN 是一款轻量级 Mesh VPN 组网工具，基于 Rust 开发，使用 WireGuard 隧道协议。它可以将分布在不同网络中的设备组成一个虚拟局域网，设备间通过加密隧道直接通信。

### 核心特性

- **Mesh 组网** — 所有节点两两直连，无需中心化路由
- **QUIC 传输** — 控制平面使用 QUIC + TLS 加密，流式多路复用
- **NAT 穿透** — 基于 STUN 的 UDP 打洞，穿透大多数 NAT 类型
- **中继回退** — 打洞失败时自动通过 Relay 转发流量
- **WireGuard 加密** — 使用 X25519 密钥交换 + ChaCha20-Poly1305 加密
- **数据持久化** — 节点注册信息保存在 SQLite，重启不丢失
- **ACL 访问控制** — 基于组的策略引擎，精细化管控节点间可见性
- **端口转发** — TCP 内网穿透，将远程服务映射到本地
- **Web 管理面板** — 浏览器实时查看节点状态、管理 ACL 策略
- **简单部署** — 一个控制服务器 + 一个中继服务器 + 客户端 CLI

### 适用场景

- 远程办公：在家访问公司内网
- 多地互联：多个办公室/机房网络互通
- 内网穿透：从公网访问 NAT 后面的服务
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
tar xzf tinyvpn-0.1.0-linux-amd64.tar.gz
cd tinyvpn-0.1.0-linux-amd64
```

### 2.3 五分钟组网

**第一步：在服务器上启动服务端**

```bash
# 启动 Relay
RELAY_ADDR=0.0.0.0:9091 ./tinyvpn-relay &

# 启动 CCS（RELAY_ADDR 设为服务器公网 IP）
CCS_ADDR=0.0.0.0:9090 RELAY_ADDR=1.2.3.4:9091 ./tinyvpn-ccs &
```

**第二步：在机器 A 上注册并连接**

```bash
./tinyvpn-cli --ccs <服务器IP>:9090 register --name office
./tinyvpn-cli --ccs <服务器IP>:9090 connect
```

输出：
```
Connecting as office (10.13.0.1)...
WireGuard interface wg-tinyvpn is up (10.13.0.1)
TinyVPN is running. Press Ctrl+C to stop.
```

**第三步：在机器 B 上注册并连接**

```bash
./tinyvpn-cli --ccs <服务器IP>:9090 register --name home
./tinyvpn-cli --ccs <服务器IP>:9090 connect
```

输出：
```
Connecting as home (10.13.0.2)...
WireGuard interface wg-tinyvpn is up (10.13.0.2)
TinyVPN is running. Press Ctrl+C to stop.
```

**第四步：验证连通**

```bash
# 在机器 A 上
ping 10.13.0.2
```

ping 通即组网成功！

**第五步：访问 Web 管理面板**

浏览器打开 `http://<服务器IP>:38080`，可实时查看节点状态。

---

## 3. 客户端命令详解

### 3.1 注册节点 — `register`

```bash
./tinyvpn-cli --ccs <服务器IP>:9090 register --name <节点名>
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
./tinyvpn-cli --ccs <服务器IP>:9090 connect
```

完整参数：
```bash
./tinyvpn-cli --ccs <服务器IP>:9090 --interface wg-tinyvpn --port 51820 connect
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--ccs` | `127.0.0.1:9090` | CCS 服务器地址 |
| `--interface` | `wg-tinyvpn` | WireGuard 接口名 |
| `--port` | `51820` | WireGuard 监听端口（UDP） |

连接流程：
1. 建立 QUIC 长连接到 CCS
2. 通过 STUN 发现公网端点
3. 上报端点到 CCS
4. 获取在线节点列表（ACL 过滤后）
5. 创建 WireGuard 虚拟网卡
6. 对每个在线节点：尝试 UDP 打洞 → 成功则直连，失败则走 Relay 中继
7. 保持心跳，按 Ctrl+C 断开

**如果端口 51820 被占用**（系统已有 WireGuard 接口），使用 `--port 51821` 指定其他端口。

### 3.3 查看状态 — `status`

```bash
./tinyvpn-cli --ccs <服务器IP>:9090 status
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
./tinyvpn-cli disconnect
```

拆除 WireGuard 接口，节点离线。如使用了自定义接口名：

```bash
./tinyvpn-cli --interface wg1 disconnect
```

### 3.5 TCP 端口转发 — `forward`

将远程 VPN 节点的 TCP 端口映射到本地，实现内网穿透。

```bash
./tinyvpn-cli forward --vpn-ip <远程VPN-IP> --remote-port <远程端口> --local-port <本地端口>
```

| 参数 | 说明 |
|------|------|
| `--vpn-ip` | 远程节点的 VPN IP（如 10.13.0.2） |
| `--remote-port` | 远程服务的 TCP 端口 |
| `--local-port` | 本地监听的 TCP 端口 |

示例：

```bash
# 转发内网 SSH（本地 2222 → 内网 10.13.0.1:22）
./tinyvpn-cli forward --vpn-ip 10.13.0.1 --remote-port 22 --local-port 2222

# 然后可以通过本地端口 SSH 到内网
ssh -p 2222 user@127.0.0.1

# 转发内网 Web（本地 8080 → 内网 10.13.0.1:80）
./tinyvpn-cli forward --vpn-ip 10.13.0.1 --remote-port 80 --local-port 8080

# 访问内网 Web 服务
curl http://127.0.0.1:8080
```

按 Ctrl+C 停止转发。

### 3.6 ACL 管理 — `acl`

管理节点的访问控制策略。

```bash
./tinyvpn-cli acl --action <操作> [参数]
```

**查看所有组和规则：**

```bash
./tinyvpn-cli acl --action list
```

**管理节点分组：**

```bash
# 将节点加入组
./tinyvpn-cli acl --action add-group --node-id <节点ID> --group-name <组名>

# 从组中移除节点
./tinyvpn-cli acl --action remove-group --node-id <节点ID> --group-name <组名>
```

**管理 ACL 规则：**

```bash
# 添加规则：允许 admin 组访问 dev 组
./tinyvpn-cli acl --action add-rule --from-group admin --to-group dev

# 移除规则
./tinyvpn-cli acl --action remove-rule --from-group admin --to-group dev
```

**ACL 规则说明：**

| 状态 | 行为 |
|------|------|
| 没有任何规则 | 所有节点互相可见（默认开放模式） |
| 存在至少一条规则 | 进入白名单模式，节点只能看到规则允许的对方 |

示例：限制只有 admin 组能看到 dev 组和 db 组：

```bash
# 给节点分组
./tinyvpn-cli acl --action add-group --node-id node-aaa --group-name admin
./tinyvpn-cli acl --action add-group --node-id node-bbb --group-name dev
./tinyvpn-cli acl --action add-group --node-id node-ccc --group-name db

# 设置规则
./tinyvpn-cli acl --action add-rule --from-group admin --to-group dev
./tinyvpn-cli acl --action add-rule --from-group admin --to-group db
```

效果：admin 能看到 dev 和 db，dev 和 db 之间互相不可见。

---

## 4. 服务端管理

### 4.1 环境变量

**CCS（控制协调服务器）：**

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CCS_ADDR` | `0.0.0.0:9090` | QUIC 监听地址 |
| `RELAY_ADDR` | `127.0.0.1:9091` | 告知客户端的 Relay 地址 |
| `WEB_ADDR` | `0.0.0.0:38080` | Web 管理面板监听地址 |

**Relay（中继服务器）：**

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `RELAY_ADDR` | `0.0.0.0:9091` | UDP 监听地址 |

### 4.2 手动启动

```bash
# 启动 Relay
RELAY_ADDR=0.0.0.0:9091 nohup ./tinyvpn-relay > /var/log/tinyvpn-relay.log 2>&1 &

# 启动 CCS（RELAY_ADDR 设为服务器的公网 IP）
CCS_ADDR=0.0.0.0:9090 RELAY_ADDR=1.2.3.4:9091 nohup ./tinyvpn-ccs > /var/log/tinyvpn-ccs.log 2>&1 &
```

### 4.3 停止服务

```bash
pkill -f tinyvpn-ccs
pkill -f tinyvpn-relay
```

### 4.4 Web 管理面板

CCS 启动后自动运行 Web 管理面板，浏览器访问：

```
http://<服务器IP>:38080
```

面板功能：

| 功能 | 说明 |
|------|------|
| 节点状态 | 显示所有节点的 VPN IP、公网端点、在线状态、所属分组 |
| ACL 组管理 | 添加/移除节点分组 |
| ACL 规则管理 | 配置组间访问策略（允许/删除） |
| 自动刷新 | 每 5 秒更新一次数据 |

---

## 5. 网络规划

### VPN 地址池

默认使用 `10.13.0.0/16`，支持最多 65534 个节点。节点 IP 按顺序分配，节点删除后 IP 会被回收重用。

### 端口清单

| 服务 | 协议 | 默认端口 | 用途 |
|------|------|----------|------|
| CCS | QUIC (UDP) | 9090 | 控制协议通信 |
| Relay | UDP | 9091 | 中继流量转发 |
| Web 面板 | HTTP (TCP) | 38080 | 管理面板 |
| WireGuard | UDP | 51820 | VPN 隧道数据 |

### 防火墙规则

```bash
# 服务器端
iptables -A INPUT -p udp --dport 9090 -j ACCEPT    # CCS (QUIC)
iptables -A INPUT -p udp --dport 9091 -j ACCEPT    # Relay
iptables -A INPUT -p tcp --dport 38080 -j ACCEPT   # Web 面板

# 所有客户端节点
iptables -A INPUT -p udp --dport 51820 -j ACCEPT   # WireGuard
```

---

## 6. 典型使用场景

### 场景一：远程办公

在家访问公司内网服务器上的 SSH 和 Web 服务。

**公司内网服务器：**
```bash
./tinyvpn-cli --ccs <公网IP>:9090 register --name office-server
./tinyvpn-cli --ccs <公网IP>:9090 connect
# → VPN IP: 10.13.0.1
```

**家里电脑：**
```bash
./tinyvpn-cli --ccs <公网IP>:9090 register --name home
./tinyvpn-cli --ccs <公网IP>:9090 connect
# → VPN IP: 10.13.0.2

# 转发公司内网 SSH
./tinyvpn-cli forward --vpn-ip 10.13.0.1 --remote-port 22 --local-port 2222

# 转发公司内网 Web
./tinyvpn-cli forward --vpn-ip 10.13.0.1 --remote-port 80 --local-port 8080
```

**访问：**
```bash
ssh -p 2222 user@127.0.0.1      # SSH 到公司
curl http://127.0.0.1:8080       # 访问公司 Web
```

### 场景二：多团队隔离

使用 ACL 策略隔离不同团队的节点可见性。

```bash
# 管理员节点可看到所有
./tinyvpn-cli acl --action add-group --node-id node-admin --group-name admin
./tinyvpn-cli acl --action add-rule --from-group admin --to-group dev
./tinyvpn-cli acl --action add-rule --from-group admin --to-group ops

# 开发组
./tinyvpn-cli acl --action add-group --node-id node-dev1 --group-name dev
./tinyvpn-cli acl --action add-group --node-id node-dev2 --group-name dev

# 运维组
./tinyvpn-cli acl --action add-group --node-id node-ops1 --group-name ops
```

效果：admin 可看到 dev 和 ops，dev 和 ops 之间互相不可见。

---

## 7. 日志与排错

### 查看日志

```bash
# 服务端日志
tail -f /var/log/tinyvpn-ccs.log
tail -f /var/log/tinyvpn-relay.log

# 客户端调试模式
RUST_LOG=tinyvpn=debug ./tinyvpn-cli --ccs <ip>:9090 connect
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
| 节点重启后丢失 | 使用旧版无持久化 | 更新到最新版（SQLite 持久化） |
| forward 连接被拒绝 | 目标服务未监听 | 确认远程端口有服务在运行 |

---

## 8. 架构概览

```
                    ┌──────────────┐
                    │   CCS 服务器  │ :9090/QUIC
                    │  节点注册     │
                    │  ACL 策略     │
                    │  SQLite 持久化│ :38080/HTTP (管理面板)
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
1. 节点注册 → CCS 分配 VPN IP 和 session token（持久化到 SQLite）
2. 节点 connect → 建立 QUIC 连接 → STUN 发现公网 IP → 上报 CCS
3. 获取 peer 列表（ACL 过滤后） → 逐个尝试 UDP 打洞
4. 打洞成功 → WireGuard 直连（加密隧道）
5. 打洞失败 → 通过 Relay 中继转发
6. 定期心跳保持在线状态（60 秒超时离线）

---

## 9. 安全说明

- 控制平面使用 QUIC + TLS 加密传输（自签名证书）
- 所有请求基于 session token 认证
- VPN 数据通过 WireGuard 内核模块加密（ChaCha20-Poly1305）
- 密钥交换使用 X25519 椭圆曲线算法
- 私钥仅存储在本地 `~/.tinyvpn/config.json`，不会传输到服务器
- ACL 策略引擎支持基于组的零信任访问控制

**已知限制：**
- TLS 使用自签名证书，暂不支持 CA/PKI 体系
- 无 NAT 类型检测，对称 NAT 环境下打洞可能失败
- 无 WireGuard 密钥自动轮换

---

## 10. 技术支持

- 日志级别设为 `debug` 后复现问题，将日志发送给开发团队
- 附带信息：操作系统版本、内核版本、网络环境（NAT 类型）
- 项目地址：https://github.com/zhoumz123/tinyvpn
