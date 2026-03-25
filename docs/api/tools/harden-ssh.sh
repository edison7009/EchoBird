#!/bin/bash
# ============================================================
# EchoBird Server Security Hardening Script
# Fully automated SSH hardening with key-only auth
# Usage: curl -fsSL https://echobird.ai/api/tools/harden-ssh.sh | bash
# ============================================================

set -e

echo "============================================="
echo "   EchoBird Server Security Hardening"
echo "   $(date '+%Y-%m-%d %H:%M:%S')"
echo "============================================="

# --- Step 1: Environment Detection ---
echo ""
echo "[1/9] Detecting environment..."
OS_ID=$(grep ^ID= /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"')
OS_VERSION=$(grep ^VERSION_ID= /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"')
CURRENT_USER=$(whoami)
CURRENT_PORT=$(grep -E "^Port " /etc/ssh/sshd_config 2>/dev/null | awk '{print $2}')
CURRENT_PORT=${CURRENT_PORT:-22}
HAS_UFW=$(command -v ufw 2>/dev/null && echo "yes" || echo "no")
HAS_FIREWALLD=$(command -v firewall-cmd 2>/dev/null && echo "yes" || echo "no")
HAS_SELINUX=$(command -v getenforce 2>/dev/null && getenforce 2>/dev/null || echo "Disabled")
echo "  OS: $OS_ID $OS_VERSION"
echo "  Current user: $CURRENT_USER"
echo "  Current SSH port: $CURRENT_PORT"
echo "  Firewall: UFW=$HAS_UFW FirewallD=$HAS_FIREWALLD"
echo "  SELinux: $HAS_SELINUX"

# --- Step 2: Generate Credentials ---
echo ""
echo "[2/9] Generating secure credentials..."
NEW_PORT=$((RANDOM % 50000 + 10001))
NEW_USER="eb_$(openssl rand -hex 4)"
echo "  New SSH port: $NEW_PORT"
echo "  New username: $NEW_USER"

# --- Step 3: Create Secure User ---
echo ""
echo "[3/9] Creating user $NEW_USER with sudo..."
useradd -m -s /bin/bash "$NEW_USER"
echo "$NEW_USER ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/$NEW_USER
chmod 440 /etc/sudoers.d/$NEW_USER
echo "  User created with sudo privileges"

# --- Step 4: Generate ED25519 Key Pair ---
echo ""
echo "[4/9] Generating ED25519 key pair..."
mkdir -p /home/$NEW_USER/.ssh
ssh-keygen -t ed25519 -f /home/$NEW_USER/.ssh/id_ed25519 -N "" -C "echobird-$NEW_USER" -q
cat /home/$NEW_USER/.ssh/id_ed25519.pub >> /home/$NEW_USER/.ssh/authorized_keys
chmod 700 /home/$NEW_USER/.ssh
chmod 600 /home/$NEW_USER/.ssh/authorized_keys
chown -R $NEW_USER:$NEW_USER /home/$NEW_USER/.ssh
echo "  Key pair generated"

# --- Step 5: Harden SSH Config ---
echo ""
echo "[5/9] Hardening SSH configuration..."
cp /etc/ssh/sshd_config /etc/ssh/sshd_config.backup.$(date +%s)
sed -i "s/^#\?Port .*/Port $NEW_PORT/" /etc/ssh/sshd_config
sed -i "s/^#\?PermitRootLogin .*/PermitRootLogin no/" /etc/ssh/sshd_config
sed -i "s/^#\?PasswordAuthentication .*/PasswordAuthentication no/" /etc/ssh/sshd_config
sed -i "s/^#\?PubkeyAuthentication .*/PubkeyAuthentication yes/" /etc/ssh/sshd_config
sed -i "s/^#\?MaxAuthTries .*/MaxAuthTries 3/" /etc/ssh/sshd_config
sed -i "s/^#\?LoginGraceTime .*/LoginGraceTime 30/" /etc/ssh/sshd_config
sed -i "s/^#\?X11Forwarding .*/X11Forwarding no/" /etc/ssh/sshd_config
grep -q "^AllowUsers" /etc/ssh/sshd_config && sed -i "s/^AllowUsers .*/AllowUsers $NEW_USER/" /etc/ssh/sshd_config || echo "AllowUsers $NEW_USER" >> /etc/ssh/sshd_config
sshd -t && echo "  SSH config validated OK" || { echo "  ERROR: SSH config invalid!"; exit 1; }

# --- Step 6: Firewall ---
echo ""
echo "[6/9] Configuring firewall for port $NEW_PORT..."
if command -v ufw &>/dev/null; then
    ufw allow $NEW_PORT/tcp comment "SSH-hardened" 2>/dev/null
    ufw --force enable 2>/dev/null
    ufw reload 2>/dev/null
    echo "  UFW: port $NEW_PORT allowed"
elif command -v firewall-cmd &>/dev/null; then
    firewall-cmd --permanent --add-port=$NEW_PORT/tcp 2>/dev/null
    firewall-cmd --reload 2>/dev/null
    echo "  FirewallD: port $NEW_PORT allowed"
else
    iptables -A INPUT -p tcp --dport $NEW_PORT -j ACCEPT 2>/dev/null
    if command -v netfilter-persistent &>/dev/null; then netfilter-persistent save 2>/dev/null; fi
    if command -v iptables-save &>/dev/null; then iptables-save > /etc/iptables/rules.v4 2>/dev/null; fi
    echo "  iptables: port $NEW_PORT allowed"
fi

# --- Step 7: SELinux ---
if [ "$HAS_SELINUX" = "Enforcing" ]; then
    echo ""
    echo "[7/9] Configuring SELinux for new SSH port..."
    semanage port -a -t ssh_port_t -p tcp $NEW_PORT 2>/dev/null || semanage port -m -t ssh_port_t -p tcp $NEW_PORT 2>/dev/null
    echo "  SELinux: port $NEW_PORT labeled as ssh_port_t"
else
    echo ""
    echo "[7/9] SELinux not active, skipping..."
fi

# --- Step 8: Install fail2ban ---
echo ""
echo "[8/9] Setting up fail2ban..."
if ! command -v fail2ban-client &>/dev/null; then
    if command -v apt-get &>/dev/null; then
        apt-get update -qq && apt-get install -y -qq fail2ban 2>/dev/null
    elif command -v yum &>/dev/null; then
        yum install -y epel-release 2>/dev/null && yum install -y fail2ban 2>/dev/null
    elif command -v dnf &>/dev/null; then
        dnf install -y fail2ban 2>/dev/null
    fi
fi
cat > /etc/fail2ban/jail.local << JAIL
[sshd]
enabled = true
port = $NEW_PORT
maxretry = 3
bantime = 3600
findtime = 600
JAIL
systemctl enable fail2ban 2>/dev/null && systemctl restart fail2ban 2>/dev/null
echo "  fail2ban configured (3 retries, 1h ban)"

# --- Step 9: Restart SSH ---
echo ""
echo "[9/9] Restarting SSH service..."
systemctl restart sshd 2>/dev/null || service sshd restart 2>/dev/null
echo "  SSH restarted on port $NEW_PORT"

# --- Summary ---
echo ""
echo "============================================="
echo "   HARDENING COMPLETE"
echo "============================================="
echo ""
echo "  New SSH Port:    $NEW_PORT"
echo "  New Username:    $NEW_USER"
echo "  Auth Method:     ED25519 key-only"
echo "  Root Login:      DISABLED"
echo "  Password Auth:   DISABLED"
echo "  fail2ban:        ACTIVE"
echo "  MaxAuthTries:    3"
echo ""
echo "=== PRIVATE KEY (SAVE THIS!) ==="
cat /home/$NEW_USER/.ssh/id_ed25519
echo ""
echo "=== END PRIVATE KEY ==="
echo ""
echo "IMPORTANT: If using cloud (AWS/GCP/Azure/Alibaba/Tencent),"
echo "also open port $NEW_PORT in your cloud security group!"
echo "============================================="
