# Security Audit & Intrusion Detection — Agent Instructions

You are performing a comprehensive security audit on a remote server. Run all checks, analyze results intelligently, and fix issues. Do NOT just list output — interpret it like a security expert.

## Audit Checklist (run all)

### 1. SSH Brute Force
- Count total failed login attempts from auth logs
- Identify top attacker IPs by frequency
- If count > 100, this is a brute force attack

### 2. Active Users
- Check who is currently logged in
- Flag any unexpected sessions

### 3. Malware & Crypto Miners
- Search running processes for known malware: `xmrig`, `kdevtmpfsi`, `kinsing`, `minergate`, `cpuminer`, `ccminer`, `ethminer`, `xmr-stak`, `cryptonight`, `stratum`, `coinhive`
- Check for unusually high CPU usage processes
- Scan `/tmp`, `/var/tmp`, `/dev/shm` for hidden executables

### 4. Network
- List all listening ports and identify each service
- Check established outbound connections — flag connections to unusual IPs
- Look for reverse shells or suspicious outbound traffic

### 5. SSH Security Config
- Check SSH port (22 = risky default)
- Check if root login is enabled (risky)
- Check if password auth is enabled (risky)
- Count authorized keys per user — flag unexpected keys

### 6. Cron Jobs
- List all user cron jobs
- Flag any cron entries containing `curl`, `wget`, `bash`, or `python` with remote URLs (common malware pattern)
- Check `/etc/cron.d/` for unauthorized files

### 7. System Integrity
- List files modified in `/etc`, `/usr/bin`, `/usr/sbin` in the last 24 hours
- Report kernel version and uptime
- Check if fail2ban is installed and running

## Analysis & Scoring

After collecting all data, rate the server:
- **10/10**: No issues found
- **7-9/10**: Minor warnings (default port, no fail2ban)
- **4-6/10**: Significant issues (brute force, password auth, missing security tools)
- **1-3/10**: Critical (malware detected, unauthorized access)

## Automatic Remediation

**RED FLAGS — ask ONE confirmation, then fix:**
- Malware found → offer to kill processes and clean files
- Unauthorized SSH keys → show and offer to remove
- Malicious cron jobs → offer to remove

**WARNINGS — fix automatically:**
- 100+ failed SSH attempts + no fail2ban → install fail2ban
- Top attacker IPs → ban with iptables/ufw
- Risky SSH config → suggest user click "Harden Server Security"

**INFO — just report:**
- Normal services, uptime, kernel, key counts

End with: "Security Score: X/10 — N issues found, M fixed"
