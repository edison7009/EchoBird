#!/bin/bash
# ============================================================
# EchoBird Network Info & NAT Traversal Setup
# Detects network topology, shows IPs, auto-installs tunnels
# Usage: curl -fsSL https://echobird.ai/api/tools/network-info.sh | bash
# ============================================================

echo "========================================="
echo "      NETWORK INFORMATION"
echo "      $(date '+%Y-%m-%d %H:%M:%S')"
echo "========================================="

# --- Internal IPs ---
echo ""
echo "--- Internal (LAN) IPs ---"
ip -4 addr show 2>/dev/null | grep inet | grep -v 127.0.0.1 | awk '{print $NF": "$2}' || ifconfig 2>/dev/null | grep "inet " | grep -v 127.0.0.1

# --- Public IP ---
echo ""
echo "--- Public (WAN) IP ---"
PUB_IP=$(curl -s --max-time 5 https://api.ipify.org 2>/dev/null || curl -s --max-time 5 https://ifconfig.me 2>/dev/null || curl -s --max-time 5 https://icanhazip.com 2>/dev/null)
echo "${PUB_IP:-Could not detect}"

# --- Hostname ---
echo ""
echo "--- Hostname & DNS ---"
hostname -f 2>/dev/null || hostname

# --- Gateway ---
echo ""
echo "--- Default Gateway ---"
ip route 2>/dev/null | grep default | head -1

# --- NAT Detection ---
echo ""
echo "--- NAT Type ---"
LAN_IP=$(ip -4 addr show 2>/dev/null | grep inet | grep -v 127.0.0.1 | head -1 | awk '{print $2}' | cut -d/ -f1)
if [ "$LAN_IP" = "$PUB_IP" ]; then
    echo "TYPE: DIRECT (no NAT) - services are directly accessible from internet"
    echo "NAT=no"
elif echo "$LAN_IP" | grep -qE "^(10\.|172\.(1[6-9]|2[0-9]|3[01])\.|192\.168\.)"; then
    echo "TYPE: BEHIND NAT - needs tunnel/port-forward for external access"
    echo "NAT=yes"
else
    echo "TYPE: UNKNOWN topology"
    echo "NAT=unknown"
fi

# --- Listening Services ---
echo ""
echo "--- Listening Services ---"
ss -tlnp 2>/dev/null | grep -v "^State" || netstat -tlnp 2>/dev/null | tail -n +3

# --- Existing Tunnels ---
echo ""
echo "--- Existing Tunnel Software ---"
command -v frps 2>/dev/null && echo "frp server (frps): $(frps --version 2>/dev/null)"
command -v frpc 2>/dev/null && echo "frp client (frpc): $(frpc --version 2>/dev/null)"
command -v cloudflared 2>/dev/null && echo "cloudflared: $(cloudflared --version 2>/dev/null | head -1)"
command -v rathole 2>/dev/null && echo "rathole: installed"
command -v ngrok 2>/dev/null && echo "ngrok: $(ngrok --version 2>/dev/null)"
pgrep -a frps 2>/dev/null && echo "  [RUNNING] frp server"
pgrep -a frpc 2>/dev/null && echo "  [RUNNING] frp client"
pgrep -a cloudflared 2>/dev/null && echo "  [RUNNING] cloudflared"

echo ""
echo "========================================="
echo "      SCAN COMPLETE"
echo "========================================="
