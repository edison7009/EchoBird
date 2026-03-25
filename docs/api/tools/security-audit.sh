#!/bin/bash
# ============================================================
# EchoBird Security Audit Script
# Comprehensive intrusion detection and malware scan
# Usage: curl -fsSL https://echobird.ai/api/tools/security-audit.sh | bash
# ============================================================

echo "============================================="
echo "     SECURITY AUDIT REPORT"
echo "     $(date '+%Y-%m-%d %H:%M:%S')"
echo "============================================="

ISSUES=0

# --- [1/10] SSH Brute Force ---
echo ""
echo "--- [1/10] Failed SSH Login Attempts ---"
FAIL_COUNT=$(grep -c "Failed password\|Invalid user" /var/log/auth.log 2>/dev/null || journalctl _COMM=sshd --no-pager 2>/dev/null | grep -ci "failed\|invalid" 2>/dev/null || echo "0")
echo "Total failed attempts: $FAIL_COUNT"
if [ "$FAIL_COUNT" -gt 100 ] 2>/dev/null; then
    echo "WARNING: High brute force activity detected!"
    ISSUES=$((ISSUES+1))
fi
grep "Failed password\|Invalid user" /var/log/auth.log 2>/dev/null | awk '{print $1,$2,$3,$9,$11}' | sort | uniq -c | sort -rn | head -10 || journalctl _COMM=sshd --no-pager -n 50 2>/dev/null | grep -i "failed\|invalid" | tail -10

# --- [2/10] Top Attacker IPs ---
echo ""
echo "--- [2/10] Brute Force IPs (top attackers) ---"
grep "Failed password" /var/log/auth.log 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="from") print $(i+1)}' | sort | uniq -c | sort -rn | head -10

# --- [3/10] Logged-in Users ---
echo ""
echo "--- [3/10] Currently Logged-in Users ---"
w 2>/dev/null || who

# --- [4/10] Malware Detection ---
echo ""
echo "--- [4/10] Known Crypto Miners & Malware ---"
MALWARE_PROCS=$(ps aux 2>/dev/null | grep -iE "xmrig|kdevtmpfsi|kinsing|minergate|cpuminer|ccminer|ethminer|xmr-stak|cryptonight|stratum|coinhive|monero|\.hidden|dbused|scout_agent" | grep -v grep)
if [ -n "$MALWARE_PROCS" ]; then
    echo "!!! CRITICAL: MALWARE DETECTED !!!"
    echo "$MALWARE_PROCS"
    ISSUES=$((ISSUES+5))
else
    echo "No known malware processes found"
fi

# --- [5/10] CPU Hogs ---
echo ""
echo "--- [5/10] High CPU Processes ---"
ps aux --sort=-%cpu 2>/dev/null | head -10

# --- [6/10] Listening Ports ---
echo ""
echo "--- [6/10] Listening Ports & Services ---"
ss -tlnp 2>/dev/null || netstat -tlnp 2>/dev/null

# --- [7/10] Outbound Connections ---
echo ""
echo "--- [7/10] Outbound Connections (established) ---"
ss -tnp state established 2>/dev/null | awk '{print $5}' | cut -d: -f1 | sort | uniq -c | sort -rn | head -15

# --- [8/10] SSH Keys ---
echo ""
echo "--- [8/10] SSH Authorized Keys ---"
echo "Root keys: $(cat /root/.ssh/authorized_keys 2>/dev/null | wc -l)"
find /home -name authorized_keys -exec sh -c 'echo "$(dirname $(dirname {}))": $(wc -l < {}) keys' \; 2>/dev/null

# --- [9/10] Cron Jobs ---
echo ""
echo "--- [9/10] Cron Jobs (all users) ---"
for user in $(cut -f1 -d: /etc/passwd 2>/dev/null); do
    CRONS=$(crontab -u "$user" -l 2>/dev/null | grep -v "^#" | grep -v "^$")
    if [ -n "$CRONS" ]; then echo "[$user] $CRONS"; fi
done
echo "Suspicious downloads in cron:"
SUSP_CRON=$(grep -rh "curl\|wget" /etc/cron* /var/spool/cron/* 2>/dev/null | grep -v "^#" | grep -iE "bash|sh|python|\.sh|pipe")
if [ -n "$SUSP_CRON" ]; then
    echo "WARNING: Suspicious cron entries found!"
    echo "$SUSP_CRON"
    ISSUES=$((ISSUES+3))
else
    echo "No suspicious cron entries"
fi

# --- [10/10] Modified System Files ---
echo ""
echo "--- [10/10] Recently Modified System Files (24h) ---"
MODIFIED=$(find /etc /usr/bin /usr/sbin /usr/local/bin -type f -mtime -1 2>/dev/null | head -20)
if [ -n "$MODIFIED" ]; then
    echo "$MODIFIED"
else
    echo "No recently modified system files"
fi

# --- Temp directory check ---
echo ""
echo "--- Hidden executables in /tmp /var/tmp /dev/shm ---"
HIDDEN_EXEC=$(find /tmp /var/tmp /dev/shm -type f -executable 2>/dev/null | head -10)
if [ -n "$HIDDEN_EXEC" ]; then
    echo "WARNING: Executable files in temp directories!"
    echo "$HIDDEN_EXEC"
    ISSUES=$((ISSUES+2))
else
    echo "No suspicious executables in temp dirs"
fi

# --- SSH Config Check ---
echo ""
echo "--- SSH Security Config ---"
SSH_PORT=$(grep -E "^Port " /etc/ssh/sshd_config 2>/dev/null | awk '{print $2}')
SSH_PORT=${SSH_PORT:-22}
ROOT_LOGIN=$(grep -E "^PermitRootLogin " /etc/ssh/sshd_config 2>/dev/null | awk '{print $2}')
PASS_AUTH=$(grep -E "^PasswordAuthentication " /etc/ssh/sshd_config 2>/dev/null | awk '{print $2}')
echo "  SSH Port: $SSH_PORT $([ "$SSH_PORT" = "22" ] && echo '(DEFAULT - RISKY!)' || echo '(custom - good)')"
echo "  Root Login: ${ROOT_LOGIN:-not set} $(echo "$ROOT_LOGIN" | grep -qi "yes" && echo '(RISKY!)' || echo '')"
echo "  Password Auth: ${PASS_AUTH:-not set} $(echo "$PASS_AUTH" | grep -qi "yes" && echo '(RISKY!)' || echo '')"
[ "$SSH_PORT" = "22" ] && ISSUES=$((ISSUES+1))
echo "$ROOT_LOGIN" | grep -qi "yes" 2>/dev/null && ISSUES=$((ISSUES+1))
echo "$PASS_AUTH" | grep -qi "yes" 2>/dev/null && ISSUES=$((ISSUES+1))

# --- System Info ---
echo ""
echo "--- System Info ---"
echo "  Kernel: $(uname -r)"
echo "  Uptime: $(uptime -p 2>/dev/null || uptime)"
echo "  Last reboot: $(who -b 2>/dev/null | awk '{print $3,$4}' || last reboot 2>/dev/null | head -1)"
echo "  fail2ban: $(command -v fail2ban-client &>/dev/null && fail2ban-client status 2>/dev/null | head -1 || echo 'NOT INSTALLED')"

# --- Score ---
echo ""
echo "============================================="
if [ "$ISSUES" -eq 0 ]; then
    echo "  SECURITY SCORE: 10/10 - ALL CLEAR"
elif [ "$ISSUES" -le 2 ]; then
    echo "  SECURITY SCORE: $((10-ISSUES))/10 - MINOR ISSUES"
elif [ "$ISSUES" -le 5 ]; then
    echo "  SECURITY SCORE: $((10-ISSUES))/10 - NEEDS ATTENTION"
else
    echo "  SECURITY SCORE: $((10-ISSUES > 0 ? 10-ISSUES : 1))/10 - CRITICAL"
fi
echo "  Issues found: $ISSUES"
echo "============================================="
