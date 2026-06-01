企业级 Full Mesh VPN / SD-WAN 组网工具，武汉云网信息科技开发，对标 Tailscale / ZeroTier / tailscale。

核心功能：

• Mesh 组网 — 设备间点对点直连，去中心化
• NAT 穿透 — 打洞成功率 99%，P2P 直连释放带宽
• 内网穿透 — TCP/UDP/HTTPS 映射到外网，基于 KCP+mTCP 协议
• 异地局域网桥接 — 多地内网互通，无需专线硬件
• ACL 权限控制 — IP/MAC/标签/协议/端口级别访问控制
• 跨平台 — Windows/macOS/Linux/Android/iOS/OpenWRT

───

如果你要开发类似产品，需要攻克的核心模块：

1. P2P 组网引擎 — WireGuard / 自研协议，NAT 打洞（STUN/TURN/ICE）
2. 协调服务器（Control Plane） — 节点注册、密钥分发、拓扑管理
3. 中继服务器（Relay） — 打洞失败时的流量转发
4. 内网穿透 — TCP/UDP 端口映射
5. ACL / 零信任策略引擎
6. 多平台客户端（TUN/TAP 虚拟网卡）
7. Web 管理面板 + 用户系统

项目开源，技术栈RUST
