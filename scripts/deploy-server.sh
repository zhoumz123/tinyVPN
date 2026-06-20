#!/bin/bash
# TinyVPN Server Deployment Script
# Deploy CCS, Relay, and Web Dashboard as systemd services

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Check if running as root
if [[ $EUID -ne 0 ]]; then
   log_error "This script must be run as root (use sudo)"
   exit 1
fi

# Configuration
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
INSTALL_DIR="/opt/tinyvpn"
LOG_DIR="/var/log/tinyvpn"
CCS_ADDR="${CCS_ADDR:-0.0.0.0:9090}"
RELAY_ADDR="${RELAY_ADDR:-0.0.0.0:9091}"
WEB_ADDR="${WEB_ADDR:-0.0.0.0:38080}"

# External relay address (for clients to connect)
# If not set, use the machine's IP or default to the relay bind address
if [[ -z "$RELAY_EXTERNAL" ]]; then
    # Try to detect the external IP
    DETECTED_IP=$(curl -s4 ifconfig.me 2>/dev/null || curl -s4 icanhazip.com 2>/dev/null || echo "")
    if [[ -n "$DETECTED_IP" ]]; then
        RELAY_EXTERNAL="$DETECTED_IP:9091"
        log_warn "Auto-detected external IP: $DETECTED_IP"
    else
        RELAY_EXTERNAL="$RELAY_ADDR"
        log_warn "Could not detect external IP, using: $RELAY_EXTERNAL"
        log_warn "Set RELAY_EXTERNAL environment variable if this is incorrect"
    fi
fi

log_info "=== TinyVPN Server Deployment ==="
log_info "Project directory: $PROJECT_DIR"
log_info "Install directory: $INSTALL_DIR"
log_info "CCS:   $CCS_ADDR"
log_info "Relay: $RELAY_ADDR (external: $RELAY_EXTERNAL)"
log_info "Web:   $WEB_ADDR"
echo ""

# Step 1: Check/Install Rust
log_info "[1/7] Checking Rust installation..."
if ! command -v rustc &> /dev/null; then
    log_warn "Rust not found. Installing Rust toolchain..."
    if ! command -v curl &> /dev/null; then
        log_error "curl not found. Please install curl first."
        exit 1
    fi
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    export PATH="$HOME/.cargo/bin:$PATH"
else
    RUST_VERSION=$(rustc --version)
    log_info "Rust found: $RUST_VERSION"
fi

# Step 2: Install system dependencies
log_info "[2/7] Installing system dependencies..."
if command -v apt-get &> /dev/null; then
    apt-get update
    apt-get install -y build-essential pkg-config libssl-dev wireguard-tools
elif command -v yum &> /dev/null; then
    yum install -y gcc make pkgconfig openssl-devel wireguard-tools
else
    log_warn "Unknown package manager. Please install: build-essential, pkg-config, libssl-dev, wireguard-tools"
fi

# Step 3: Build binaries
log_info "[3/7] Building TinyVPN components..."
cd "$PROJECT_DIR"
if [[ -d "$INSTALL_DIR" && -f "$INSTALL_DIR/bin/tinyvpn-ccs" ]]; then
    log_info "Existing binaries found. Rebuilding..."
fi

cargo build --release --workspace

BUILD_TIME=$(cargo build --release --workspace 2>&1 | grep "Finished" || true)
log_info "Build complete: $BUILD_TIME"

# Step 4: Create install directories
log_info "[4/7] Setting up directories..."
mkdir -p "$INSTALL_DIR/bin"
mkdir -p "$LOG_DIR"
chown -R root:root "$INSTALL_DIR"
chmod 755 "$INSTALL_DIR/bin"

# Step 5: Copy binaries
log_info "[5/7] Installing binaries..."
cp -f "$PROJECT_DIR/target/release/tinyvpn-ccs" "$INSTALL_DIR/bin/"
cp -f "$PROJECT_DIR/target/release/tinyvpn-relay" "$INSTALL_DIR/bin/"
cp -f "$PROJECT_DIR/target/release/tinyvpn-cli" "$INSTALL_DIR/bin/"
chmod +x "$INSTALL_DIR/bin/"*

log_info "Binaries installed to $INSTALL_DIR/bin/"

# Step 6: Create systemd services
log_info "[6/7] Installing systemd services..."

# tinyvpn-relay.service
cat > /etc/systemd/system/tinyvpn-relay.service << 'EOF'
[Unit]
Description=TinyVPN Relay Server
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/tinyvpn
Environment="RELAY_ADDR=0.0.0.0:9091"
ExecStart=/opt/tinyvpn/bin/tinyvpn-relay
Restart=on-failure
RestartSec=5s
StandardOutput=append:/var/log/tinyvpn/relay.log
StandardError=append:/var/log/tinyvpn/relay.log

[Install]
WantedBy=multi-user.target
EOF

# tinyvpn-ccs.service
cat > /etc/systemd/system/tinyvpn-ccs.service << EOF
[Unit]
Description=TinyVPN Control Coordination Server
After=network.target tinyvpn-relay.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/tinyvpn
Environment="CCS_ADDR=$CCS_ADDR"
Environment="RELAY_ADDR=$RELAY_EXTERNAL"
Environment="WEB_ADDR=$WEB_ADDR"
ExecStart=/opt/tinyvpn/bin/tinyvpn-ccs
Restart=on-failure
RestartSec=5s
StandardOutput=append:/var/log/tinyvpn/ccs.log
StandardError=append:/var/log/tinyvpn/ccs.log

[Install]
WantedBy=multi-user.target
EOF

# Reload systemd
systemctl daemon-reload

# Step 7: Configure firewall (if applicable)
log_info "[7/7] Configuring firewall..."

# Get the port numbers from addresses
CCS_PORT=$(echo "$CCS_ADDR" | cut -d: -f2)
RELAY_PORT=$(echo "$RELAY_ADDR" | cut -d: -f2)
WEB_PORT=$(echo "$WEB_ADDR" | cut -d: -f2)

if command -v firewall-cmd &> /dev/null; then
    # firewalld
    if systemctl is-active --quiet firewalld; then
        firewall-cmd --permanent --add-port="$CCS_PORT/tcp" 2>/dev/null || true
        firewall-cmd --permanent --add-port="$RELAY_PORT/udp" 2>/dev/null || true
        firewall-cmd --permanent --add-port="$WEB_PORT/tcp" 2>/dev/null || true
        firewall-cmd --permanent --add-port=51820/udp 2>/dev/null || true  # WireGuard
        firewall-cmd --reload 2>/dev/null || true
        log_info "Firewalld rules added"
    fi
elif command -v ufw &> /dev/null; then
    # ufw (Ubuntu)
    ufw allow "$CCS_PORT/tcp" 2>/dev/null || true
    ufw allow "$RELAY_PORT/udp" 2>/dev/null || true
    ufw allow "$WEB_PORT/tcp" 2>/dev/null || true
    ufw allow 51820/udp 2>/dev/null || true  # WireGuard
    log_info "UFW rules added"
else
    log_warn "No firewall detected. Please ensure ports $CCS_PORT/tcp, $RELAY_PORT/udp, $WEB_PORT/tcp are open."
fi

# Enable and start services
log_info "Enabling and starting services..."
systemctl enable tinyvpn-relay.service
systemctl enable tinyvpn-ccs.service

# Stop existing services if running
systemctl stop tinyvpn-relay.service 2>/dev/null || true
systemctl stop tinyvpn-ccs.service 2>/dev/null || true

# Start relay first
systemctl start tinyvpn-relay.service
sleep 2
# Start CCS
systemctl start tinyvpn-ccs.service
sleep 2

# Check status
echo ""
log_info "=== Deployment Complete ==="
echo ""
echo "Service Status:"
echo "----------------------------------------"

if systemctl is-active --quiet tinyvpn-relay.service; then
    echo -e "  Relay: ${GREEN}running${NC}"
else
    echo -e "  Relay: ${RED}failed${NC}"
fi

if systemctl is-active --quiet tinyvpn-ccs.service; then
    echo -e "  CCS:   ${GREEN}running${NC}"
else
    echo -e "  CCS:   ${RED}failed${NC}"
fi

echo ""
echo "Ports to open on your firewall/router:"
echo "  $CCS_PORT/tcp   - CCS (control)"
echo "  $RELAY_PORT/udp - Relay (data forwarding)"
echo "  $WEB_PORT/tcp   - Web Dashboard"
echo "  51820/udp       - WireGuard (if running client on server)"
echo ""
echo "Client connection commands:"
echo "----------------------------------------"
echo "  # Register a new node"
echo "  $INSTALL_DIR/bin/tinyvpn-cli --ccs $(echo "$CCS_ADDR" | sed 's/0.0.0.0/'"$RELAY_EXTERNAL"'/' | cut -d: -f1):$CCS_PORT register --name <node-name>"
echo ""
echo "  # Connect to VPN"
echo "  $INSTALL_DIR/bin/tinyvpn-cli --ccs $(echo "$CCS_ADDR" | sed 's/0.0.0.0/'"$RELAY_EXTERNAL"'/' | cut -d: -f1):$CCS_PORT connect"
echo ""
echo "Web Dashboard: http://$(echo "$RELAY_EXTERNAL" | cut -d: -f1):$WEB_PORT"
echo ""
echo "Logs:"
echo "  Relay: tail -f $LOG_DIR/relay.log"
echo "  CCS:   tail -f $LOG_DIR/ccs.log"
echo ""
echo "Management:"
echo "  systemctl status tinyvpn-ccs tinyvpn-relay"
echo "  systemctl restart tinyvpn-ccs tinyvpn-relay"
echo ""
