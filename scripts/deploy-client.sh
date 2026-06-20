#!/bin/bash
# TinyVPN Client Deployment Script
# Install CLI, register node, configure auto-start

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

# Parse arguments
if [[ $# -lt 2 ]]; then
    log_error "Usage: $0 <ccs-address> <node-name> [wg-port]"
    echo ""
    echo "Arguments:"
    echo "  ccs-address   - CCS server address (e.g., 47.115.35.7:9090)"
    echo "  node-name     - Unique name for this node (e.g., client-node)"
    echo "  wg-port       - WireGuard listen port (default: 51820)"
    echo ""
    echo "Example:"
    echo "  $0 47.115.35.7:9090 my-laptop"
    echo "  $0 47.115.35.7:9090 office-pc 51821"
    exit 1
fi

CCS_ADDR="$1"
NODE_NAME="$2"
WG_PORT="${3:-51820}"

# Configuration
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
INSTALL_DIR="/opt/tinyvpn"
LOG_DIR="/var/log/tinyvpn"
CONFIG_DIR="/root/.tinyvpn"

log_info "=== TinyVPN Client Deployment ==="
log_info "CCS Server:  $CCS_ADDR"
log_info "Node Name:   $NODE_NAME"
log_info "WG Port:     $WG_PORT"
echo ""

# Step 1: Install dependencies
log_info "[1/6] Installing system dependencies..."

if command -v apt-get &> /dev/null; then
    apt-get update
    apt-get install -y wireguard-tools
elif command -v yum &> /dev/null; then
    yum install -y wireguard-tools
else
    log_warn "Unknown package manager. Please install wireguard-tools manually."
fi

# Check for WireGuard
if ! command -v wg &> /dev/null; then
    log_error "WireGuard tools not found. Please install wireguard-tools package."
    exit 1
fi

# Step 2: Install CLI binary
log_info "[2/6] Installing TinyVPN CLI..."

mkdir -p "$INSTALL_DIR/bin"
mkdir -p "$LOG_DIR"

# Check if we need to build or copy
if [[ -f "$PROJECT_DIR/target/release/tinyvpn-cli" ]]; then
    # Copy from local build
    cp -f "$PROJECT_DIR/target/release/tinyvpn-cli" "$INSTALL_DIR/bin/"
    log_info "Copied from local build"
elif [[ -f "$INSTALL_DIR/bin/tinyvpn-cli" ]]; then
    log_info "Using existing binary at $INSTALL_DIR/bin/tinyvpn-cli"
else
    # Need to build - check for Rust
    if ! command -v cargo &> /dev/null; then
        log_warn "Rust/Cargo not found. Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        export PATH="$HOME/.cargo/bin:$PATH"
    fi

    log_info "Building TinyVPN CLI (this may take a while)..."
    cd "$PROJECT_DIR"
    cargo build --release -p tinyvpn-cli
    cp -f "$PROJECT_DIR/target/release/tinyvpn-cli" "$INSTALL_DIR/bin/"
fi

chmod +x "$INSTALL_DIR/bin/tinyvpn-cli"
log_info "CLI installed to $INSTALL_DIR/bin/tinyvpn-cli"

# Step 3: Check if already registered
log_info "[3/6] Checking registration status..."

if [[ -f "$CONFIG_DIR/config.json" ]]; then
    log_warn "Node already registered. Using existing config."
    EXISTING_NAME=$(grep -o '"name"[[:space:]]*:[[:space:]]*"[^"]*"' "$CONFIG_DIR/config.json" | cut -d'"' -f4)
    log_info "Existing node: $EXISTING_NAME"
    REGISTERED="yes"
else
    REGISTERED="no"
    log_info "Node not registered. Will register with CCS."
fi

# Step 4: Register node (if needed)
if [[ "$REGISTERED" == "no" ]]; then
    log_info "[4/6] Registering node $NODE_NAME with $CCS_ADDR..."

    REGISTER_OUTPUT=$("$INSTALL_DIR/bin/tinyvpn-cli" --ccs "$CCS_ADDR" register --name "$NODE_NAME" 2>&1)
    REGISTER_EXIT=$?

    if [[ $REGISTER_EXIT -eq 0 ]]; then
        log_info "Registration successful!"
        echo "$REGISTER_OUTPUT"
        # Extract VPN IP from output
        VPN_IP=$(echo "$REGISTER_OUTPUT" | grep -o 'VPN IP:.*' | cut -d: -f2 | xargs || echo "")
    else
        log_error "Registration failed!"
        echo "$REGISTER_OUTPUT"
        exit 1
    fi
else
    log_info "Skipping registration (already done)"
    # Get VPN IP from existing config
    VPN_IP=$(grep -o '"vpn_ip"[[:space:]]*:[[:space:]]*"[^"]*"' "$CONFIG_DIR/config.json" | cut -d'"' -f4 || echo "")
fi

# Step 5: Create systemd service
log_info "[5/6] Configuring auto-start service..."

cat > /etc/systemd/system/tinyvpn-client.service << EOF
[Unit]
Description=TinyVPN Client - $NODE_NAME
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/tinyvpn
Environment="RUST_LOG=info"
ExecStart=$INSTALL_DIR/bin/tinyvpn-cli --ccs $CCS_ADDR connect
Restart=on-failure
RestartSec=10s
StandardOutput=append:$LOG_DIR/client.log
StandardError=append:$LOG_DIR/client.log

# Hardening
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable tinyvpn-client.service

# Step 6: Start service and verify
log_info "[6/6] Starting VPN connection..."

# Stop if already running
systemctl stop tinyvpn-client.service 2>/dev/null || true
sleep 2

# Start the service
systemctl start tinyvpn-client.service

# Wait for interface to come up
log_info "Waiting for WireGuard interface to initialize..."
WAIT_COUNT=0
MAX_WAIT=30

while [[ $WAIT_COUNT -lt $MAX_WAIT ]]; do
    if ip link show wg-tinyvpn &>/dev/null; then
        log_info "WireGuard interface 'wg-tinyvpn' is up!"
        break
    fi
    sleep 1
    ((WAIT_COUNT++))
done

if [[ $WAIT_COUNT -ge $MAX_WAIT ]]; then
    log_warn "Interface did not come up within $MAX_WAIT seconds. Check logs."
fi

# Final status display
echo ""
log_info "=== Deployment Complete ==="
echo ""

# Get VPN IP if not already set
if [[ -z "$VPN_IP" ]]; then
    VPN_IP=$(grep -o '"vpn_ip"[[:space:]]*:[[:space:]]*"[^"]*"' "$CONFIG_DIR/config.json" | cut -d'"' -f4 || echo "")
fi

echo "Node Information:"
echo "----------------------------------------"
echo "  Name:     $NODE_NAME"
echo "  VPN IP:   ${VPN_IP:-unknown}"
echo "  WG Port:  $WG_PORT"
echo ""

if systemctl is-active --quiet tinyvpn-client.service; then
    echo -e "Service Status: ${GREEN}running${NC}"
else
    echo -e "Service Status: ${RED}not running${NC}"
fi

if ip link show wg-tinyvpn &>/dev/null; then
    echo -e "WG Interface:   ${GREEN}up${NC} (wg-tinyvpn)"
    echo ""
    echo "WireGuard Status:"
    wg show wg-tinyvpn 2>/dev/null || echo "  (unable to retrieve status)"
else
    echo -e "WG Interface:   ${RED}down${NC} or not created"
fi

echo ""
echo "Management Commands:"
echo "----------------------------------------"
echo "  Check status:    systemctl status tinyvpn-client"
echo "  View logs:       tail -f $LOG_DIR/client.log"
echo "  Stop VPN:        systemctl stop tinyvpn-client"
echo "  Restart VPN:     systemctl restart tinyvpn-client"
echo "  Manual connect:  $INSTALL_DIR/bin/tinyvpn-cli --ccs $CCS_ADDR connect"
echo "  View peers:      $INSTALL_DIR/bin/tinyvpn-cli --ccs $CCS_ADDR status"
echo ""
