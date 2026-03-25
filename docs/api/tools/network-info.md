# Network Info & NAT Traversal — Agent Instructions

You are helping the user understand their server's network topology and set up external access if needed. Execute everything autonomously.

## Step 1: Gather Network Info

Collect and display:
- All internal (LAN) IP addresses
- Public (WAN) IP (try ipify.org, ifconfig.me, icanhazip.com)
- Hostname
- Default gateway
- All listening services (port + process name)

## Step 2: Detect NAT Type

Compare internal IP vs public IP:
- **Same** → Direct public IP, no NAT. Services are directly accessible.
- **Internal is private (10.x / 172.16-31.x / 192.168.x)** → Behind NAT, needs tunnel for external access.

## Step 3: Check Existing Tunnel Software

Look for:
- frp (frps/frpc) — check if binary exists or process is running
- cloudflared — check if installed
- rathole, ngrok — check if present

Report what's already available.

## Step 4: Act Based on Results

**If Direct Public IP:**
- Tell user services are accessible at `<public_ip>:<port>`
- Recommend firewall if not active
- Done

**If Behind NAT:**
- Ask ONE question: "Want me to set up external access so you can reach this server from anywhere?"
- If yes, auto-select the best tool (do NOT ask user to choose):

### Decision Tree:
1. **frp already installed** → configure it (user has infrastructure)
2. **cloudflared installed** → use Cloudflare Tunnel
3. **Nothing installed** → install frp from GitHub releases (most universal, no third-party account needed)

### frp Setup:
- Download latest release for the correct architecture
- Detect role: is this server public-facing (set up frps) or behind NAT (set up frpc)?
- If frpc: ask user for their public server's IP, then configure tunnel for SSH + detected services
- Create systemd service for auto-start
- Verify tunnel works

### Cloudflare Tunnel Setup:
- Install cloudflared binary
- Guide through `cloudflared tunnel login` → create tunnel → route DNS → run as service
- This requires a Cloudflare account and domain

After setup, present:
- External access URL/IP:port for each tunneled service
- How to add more services later
