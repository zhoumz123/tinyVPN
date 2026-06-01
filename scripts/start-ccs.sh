#!/bin/bash
# TinyVPN CCS + Relay 一键启动脚本
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="$SCRIPT_DIR/../bin"

# 默认配置
CCS_ADDR="${CCS_ADDR:-0.0.0.0:9090}"
RELAY_ADDR="${RELAY_ADDR:-0.0.0.0:9091}"
LOG_DIR="${LOG_DIR:-/var/log/tinyvpn}"

mkdir -p "$LOG_DIR"

echo "=== TinyVPN Server Starting ==="
echo "CCS:   $CCS_ADDR (TCP)"
echo "Relay: $RELAY_ADDR (UDP)"
echo "Logs:  $LOG_DIR/"
echo ""

# 启动 Relay
echo "[1/2] Starting Relay..."
RELAY_ADDR="$RELAY_ADDR" nohup "$BIN_DIR/tinyvpn-relay" > "$LOG_DIR/relay.log" 2>&1 &
RELAY_PID=$!
echo "      Relay PID: $RELAY_PID"

sleep 1

# 获取 Relay 实际监听地址
RELAY_EXTERNAL="${RELAY_EXTERNAL:-$RELAY_ADDR}"

# 启动 CCS
echo "[2/2] Starting CCS..."
CCS_ADDR="$CCS_ADDR" RELAY_ADDR="$RELAY_EXTERNAL" nohup "$BIN_DIR/tinyvpn-ccs" > "$LOG_DIR/ccs.log" 2>&1 &
CCS_PID=$!
echo "      CCS PID:   $CCS_PID"

echo ""
echo "=== Started ==="
echo "Relay PID: $RELAY_PID  |  Log: $LOG_DIR/relay.log"
echo "CCS PID:   $CCS_PID    |  Log: $LOG_DIR/ccs.log"
echo ""
echo "Client connect command:"
echo "  ./tinyvpn-cli --ccs <this-server-ip>:9090 register --name <name>"
echo "  ./tinyvpn-cli --ccs <this-server-ip>:9090 connect"
echo ""
echo "To stop: kill $RELAY_PID $CCS_PID"
