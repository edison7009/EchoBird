# SSH Server Hardening — Agent Instructions

You are hardening a remote Linux server's SSH access. Execute everything autonomously via `shell_exec`. Adapt to the server's OS and tooling — do NOT blindly copy commands.

## Phase 1: Assess (run first, adapt everything after)

Check the current state:
- What OS and version? (`cat /etc/os-release`)
- What is the current SSH port? (`grep Port /etc/ssh/sshd_config`)
- Is root login enabled? Is password auth enabled?
- Which firewall is running? (ufw / firewalld / iptables / none)
- Is SELinux enforcing? (`getenforce`)
- Is fail2ban installed?

## Phase 2: Harden (execute all steps)

1. **Random SSH port**: Pick a random port between 10000–60000. Avoid well-known ports.
2. **Create secure user**: Name format `eb_<random>` (e.g., `eb_a3f7c912`). Give it sudo access via `/etc/sudoers.d/`.
3. **Generate ED25519 key pair** for the new user. Put the public key in `authorized_keys`.
4. **Lock down sshd_config**:
   - Change Port to the new port
   - `PermitRootLogin no`
   - `PasswordAuthentication no`
   - `PubkeyAuthentication yes`
   - `MaxAuthTries 3`
   - `LoginGraceTime 30`
   - `X11Forwarding no`
   - `AllowUsers <new_user>`
   - Validate with `sshd -t` before restarting
5. **Firewall**: Open the new port. Auto-detect ufw/firewalld/iptables and persist rules.
6. **SELinux**: If enforcing, label the new port as `ssh_port_t` via `semanage`.
7. **fail2ban**: Install if missing (auto-detect apt/yum/dnf). Configure jail for the new SSH port: 3 retries, 1 hour ban.
8. **Backup first**: Always `cp sshd_config` with timestamp before editing.
9. **Restart SSH**: Only after `sshd -t` passes.

## Phase 3: Deliver Credentials

Show the user:
- New SSH Port
- New Username
- Private key (full content — user must save it)
- Summary of what was changed

Tell the user to add a new server in the right panel with the new port/username/key.

**Cloud reminder**: If the server is on AWS/GCP/Azure/Alibaba/Tencent Cloud, remind the user to also open the new port in their cloud console security group.

## Phase 4: Lock Down Old Access (after user switches)

When the user confirms they connected via the new server:
1. Verify connection works
2. Disable old user: `usermod -L` + set shell to `/usr/sbin/nologin` (ask one confirmation)
3. Validate and restart SSH
4. Report final security status
