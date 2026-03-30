# Deploy Local SSH — Agent Instructions

You are helping the user enable SSH access on **this machine** so that other devices (phone, tablet, another PC) can connect to it via EchoBird Mother Agent.

**Goal**: Install and configure an SSH server on this machine, verify it works, then give the user their connection credentials.

---

## Phase 1: Detect OS

Run the following to detect the operating system:

```bash
uname -s 2>/dev/null || echo "WINDOWS"
```

- If output contains `Linux` → follow **Linux path**
- If output contains `Darwin` → follow **macOS path**
- If output is `WINDOWS` or command fails → follow **Windows path**

---

## Phase 2: Deploy SSH Server

### Linux

1. **Detect package manager and install OpenSSH server**:
   ```bash
   if command -v apt-get &>/dev/null; then
     apt-get update -qq && apt-get install -y openssh-server
   elif command -v yum &>/dev/null; then
     yum install -y openssh-server
   elif command -v dnf &>/dev/null; then
     dnf install -y openssh-server
   elif command -v pacman &>/dev/null; then
     pacman -Sy --noconfirm openssh
   fi
   ```

2. **Enable and start**:
   ```bash
   systemctl enable ssh 2>/dev/null || systemctl enable sshd 2>/dev/null
   systemctl start ssh 2>/dev/null || systemctl start sshd 2>/dev/null
   systemctl is-active ssh 2>/dev/null || systemctl is-active sshd 2>/dev/null
   ```

3. **Open firewall** (if active):
   ```bash
   if command -v ufw &>/dev/null && ufw status | grep -q active; then
     ufw allow 22/tcp
   elif command -v firewall-cmd &>/dev/null; then
     firewall-cmd --add-service=ssh --permanent && firewall-cmd --reload
   fi
   ```

4. **Get connection info**:
   ```bash
   hostname -I | awk '{print $1}'   # local IP
   whoami                           # username
   ```

### macOS

1. **Enable Remote Login** (built-in OpenSSH):
   ```bash
   sudo systemsetup -setremotelogin on
   ```
   Or via command line:
   ```bash
   sudo launchctl load -w /System/Library/LaunchDaemons/ssh.plist 2>/dev/null
   ```

2. **Verify it's running**:
   ```bash
   sudo systemsetup -getremotelogin
   ```

3. **Get connection info**:
   ```bash
   ipconfig getifaddr en0 2>/dev/null || ipconfig getifaddr en1 2>/dev/null
   whoami
   ```

### Windows

Windows has a built-in OpenSSH Server (available in Windows 10 1809+ and Windows 11).

1. **Install OpenSSH Server** (if not already installed):
   ```powershell
   Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
   ```

2. **Start the service and set to auto-start**:
   ```powershell
   Start-Service sshd
   Set-Service -Name sshd -StartupType Automatic
   ```

3. **Verify firewall rule** (auto-created, but confirm):
   ```powershell
   Get-NetFirewallRule -Name *ssh* | Select-Object Name, Enabled
   ```
   If no rule exists, create it:
   ```powershell
   New-NetFirewallRule -Name sshd -DisplayName 'OpenSSH Server (sshd)' -Enabled True -Direction Inbound -Protocol TCP -Action Allow -LocalPort 22
   ```

4. **Get connection info**:
   ```powershell
   (Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notmatch '^127\.' -and $_.IPAddress -notmatch '^169\.' } | Select-Object -First 1).IPAddress
   $env:USERNAME
   ```

---

## Phase 3: Verify SSH is Listening

Confirm port 22 is open locally:

```bash
# Linux/macOS
ss -tulpn 2>/dev/null | grep :22 || netstat -an | grep :22 || echo "Port check failed"

# Windows (PowerShell)
Test-NetConnection -ComputerName localhost -Port 22 | Select-Object TcpTestSucceeded
```

---

## Phase 4: Report to User

Deliver the result in the user's language, clearly and concisely. Include:

1. ✅ **SSH is now enabled** (or already was)
2. The exact credentials they need to add to EchoBird:
   - **IP Address**: `[detected local IP]`
   - **Port**: `22`
   - **Username**: `[detected username]`
   - **Password**: their current login password (you cannot retrieve it — tell them to use their Windows/macOS/Linux login password)

3. Clear next step:
   > "Now go to **Mother Agent** on your phone → click **+** to add a server → enter these credentials. You'll be able to control this machine from your phone."

4. **Important note for Windows users**: If they have a Windows login PIN (not a password), they may need to set a traditional password first via: Settings → Accounts → Sign-in options → Password.

---

## Edge Cases

- **SSH already running**: Skip install steps, just report connection info. Say "SSH is already enabled on this machine."
- **No sudo/admin access**: Inform the user that admin privileges are required and guide them to re-run with elevated permissions.
- **Cloud/VPS server**: If this is a cloud server, SSH is likely already configured by the provider. Report existing connection info without modification.
- **Dynamic IP (home network)**: Mention that the IP address may change if on a home network without a static IP, and suggest checking the IP each time or setting up a static local IP.
