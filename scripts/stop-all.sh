#!/bin/bash
# TinyVPN 停止脚本
echo "Stopping TinyVPN services..."
pkill -f tinyvpn-ccs 2>/dev/null && echo "  CCS stopped"    || echo "  CCS not running"
pkill -f tinyvpn-relay 2>/dev/null && echo "  Relay stopped" || echo "  Relay not running"
echo "Done."
