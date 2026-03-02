// SS Proxy Server �?mirrors old ssProxyServer.ts
// HTTP CONNECT proxy with Shadowsocks AEAD tunnel support
//
// Architecture:
//   Client -> HTTP CONNECT -> ProxyServer -> [SS Tunnel | Direct] -> Target
//
// The proxy listens on 127.0.0.1:0, accepts CONNECT requests,
// and either tunnels via Shadowsocks or connects directly based on host rules.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::models::config::ProxyRule;
use crate::models::model::SSNode;
use crate::services::crypto;
use crate::utils::platform::echobird_dir;

// ─── Constants ───

const PAYLOAD_SIZE_MASK: usize = 0x3FFF;

/// Default proxy rules
fn default_rules() -> Vec<ProxyRule> {
    vec![
        ProxyRule { pattern: "*.openai.com".to_string(), enabled: true },
        ProxyRule { pattern: "*.anthropic.com".to_string(), enabled: true },
        ProxyRule { pattern: "*.googleapis.com".to_string(), enabled: true },
        ProxyRule { pattern: "*.google.com".to_string(), enabled: true },
    ]
}

// ─── Rule management ───

/// Get proxy config dir
fn proxy_config_dir() -> std::path::PathBuf {
    echobird_dir().join("proxy")
}

/// Load proxy rules from disk
pub fn get_proxy_rules() -> Vec<ProxyRule> {
    let rules_path = proxy_config_dir().join("rules.json");
    if rules_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&rules_path) {
            if let Ok(rules) = serde_json::from_str::<Vec<ProxyRule>>(&content) {
                return rules;
            }
        }
    }
    default_rules()
}

/// Save proxy rules to disk
pub fn save_proxy_rules(rules: &[ProxyRule]) {
    let dir = proxy_config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("rules.json");
    let content = serde_json::to_string_pretty(rules).unwrap_or_default();
    let _ = std::fs::write(path, content);
}

/// Check if hostname matches proxy rules
pub fn should_proxy(hostname: &str) -> bool {
    let rules = get_proxy_rules();
    for rule in &rules {
        if !rule.enabled {
            continue;
        }
        if rule.pattern.starts_with("*.") {
            let domain = &rule.pattern[2..];
            if hostname.ends_with(domain) {
                return true;
            }
        }
        if hostname == rule.pattern
            || (rule.pattern.starts_with('.') && hostname.ends_with(&rule.pattern))
        {
            return true;
        }
    }
    false
}

// ─── Shadowsocks AEAD Stream ───

/// Connect to SS server and create encrypted tunnel to target
async fn connect_shadowsocks(
    config: &SSNode,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream, String> {
    let addr = format!("{}:{}", config.server, config.port);
    let mut socket = TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("Failed to connect to SS server {}: {}", addr, e))?;

    let cipher_info = crypto::get_cipher_info(&config.cipher.to_lowercase())
        .ok_or_else(|| format!("Unsupported cipher: {}", config.cipher))?;

    let psk = crypto::kdf(&config.password, cipher_info.key_len);

    // Generate client salt and derive encryption key
    let enc_salt: Vec<u8> = (0..cipher_info.salt_len).map(|_| rand::random::<u8>()).collect();
    let enc_key = crypto::hkdf_sha1(&psk, &enc_salt, crypto::HKDF_INFO, cipher_info.key_len);
    let mut enc_nonce = vec![0u8; cipher_info.nonce_len];

    // Send salt
    socket.write_all(&enc_salt).await.map_err(|e| format!("Salt write error: {}", e))?;

    // Build target address payload: [0x03, host_len, host_bytes, port_be16]
    let host_bytes = target_host.as_bytes();
    let mut target_addr = Vec::with_capacity(4 + host_bytes.len());
    target_addr.push(0x03); // Domain type
    target_addr.push(host_bytes.len() as u8);
    target_addr.extend_from_slice(host_bytes);
    target_addr.push((target_port >> 8) as u8);
    target_addr.push((target_port & 0xFF) as u8);

    // Encrypt and send target address as first payload chunk
    ss_write_payload(&mut socket, &target_addr, &enc_key, &mut enc_nonce, &cipher_info).await?;

    log::info!("[Proxy] SS connection established to {} via {}", target_host, addr);
    Ok(socket)
}

/// Write a payload using SS AEAD chunked format
async fn ss_write_payload(
    socket: &mut TcpStream,
    data: &[u8],
    key: &[u8],
    nonce: &mut [u8],
    cipher_info: &crypto::CipherInfo,
) -> Result<(), String> {
    let mut offset = 0;
    while offset < data.len() {
        let chunk_len = std::cmp::min(data.len() - offset, PAYLOAD_SIZE_MASK);
        let payload = &data[offset..offset + chunk_len];

        // 1. Encrypt length (2 bytes, big-endian)
        let len_bytes = [(chunk_len >> 8) as u8, (chunk_len & 0xFF) as u8];
        let enc_len = crypto::encrypt_aead(&len_bytes, key, nonce, cipher_info.algorithm)
            .map_err(|e| format!("Length encrypt error: {}", e))?;
        socket.write_all(&enc_len).await.map_err(|e| format!("Write error: {}", e))?;

        // 2. Encrypt payload
        let enc_payload = crypto::encrypt_aead(payload, key, nonce, cipher_info.algorithm)
            .map_err(|e| format!("Payload encrypt error: {}", e))?;
        socket.write_all(&enc_payload).await.map_err(|e| format!("Write error: {}", e))?;

        offset += chunk_len;
    }
    Ok(())
}

// ─── Proxy Server ───

/// Proxy server state
pub struct ProxyServer {
    port: u16,
    config: Option<SSNode>,
    host_rules: Arc<Mutex<HashMap<String, SSNode>>>,
}

impl ProxyServer {
    pub fn new() -> Self {
        Self {
            port: 0,
            config: None,
            host_rules: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start the proxy on a random port
    pub async fn start(&mut self, config: Option<SSNode>) -> Result<u16, String> {
        self.config = config;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind proxy: {}", e))?;

        let addr = listener.local_addr().map_err(|e| format!("Address error: {}", e))?;
        self.port = addr.port();
        log::info!("[Proxy] Server started on port {}", self.port);

        let host_rules = self.host_rules.clone();
        let default_config = self.config.clone();

        // Spawn connection handler
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        let rules = host_rules.clone();
                        let cfg = default_config.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, rules, cfg).await {
                                log::error!("[Proxy] Connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        log::error!("[Proxy] Accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(self.port)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn add_host_rule(&self, hostname: String, ss_node: SSNode) {
        let mut rules = self.host_rules.lock().await;
        log::info!("[Proxy] Added host rule: {} -> SS:{}", hostname, ss_node.name);
        rules.insert(hostname, ss_node);
    }

    pub async fn clear_host_rules(&self) {
        self.host_rules.lock().await.clear();
        log::info!("[Proxy] Cleared host rules");
    }
}

/// Handle a single HTTP CONNECT connection
async fn handle_connection(
    mut client: TcpStream,
    host_rules: Arc<Mutex<HashMap<String, SSNode>>>,
    default_config: Option<SSNode>,
) -> Result<(), String> {
    // Read HTTP request line
    let mut buf = vec![0u8; 4096];
    let n = client.read(&mut buf).await.map_err(|e| format!("Read error: {}", e))?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse CONNECT request
    let first_line = request.lines().next().unwrap_or("");
    if !first_line.starts_with("CONNECT") {
        let _ = client.write_all(b"HTTP/1.1 501 Not Implemented\r\n\r\n").await;
        return Ok(());
    }

    // Extract target host:port from "CONNECT host:port HTTP/1.1"
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
        return Ok(());
    }

    let target = parts[1];
    let (target_host, target_port) = parse_host_port(target)?;

    log::info!("[Proxy] CONNECT request for {}:{}", target_host, target_port);

    // Determine routing: SS tunnel or direct
    let rules = host_rules.lock().await;
    let ss_node = rules.get(&target_host).cloned();
    drop(rules);

    let use_ss = ss_node.is_some() || (default_config.is_some() && should_proxy(&target_host));

    if use_ss {
        let node = ss_node.or(default_config).unwrap();
        log::info!("[Proxy] Routing {} via SS: {}", target_host, node.name);

        match connect_shadowsocks(&node, &target_host, target_port).await {
            Ok(mut server) => {
                // Send 200 to client
                client
                    .write_all(b"HTTP/1.1 200 Connection Established\r\nProxy-agent: Echobird-SS-Proxy\r\n\r\n")
                    .await
                    .map_err(|e| format!("Write error: {}", e))?;

                // Bidirectional copy
                let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
            }
            Err(e) => {
                log::error!("[Proxy] SS connection failed for {}: {}", target_host, e);
                let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
            }
        }
    } else {
        // Direct connection
        log::info!("[Proxy] Routing {} DIRECT", target_host);
        let addr = format!("{}:{}", target_host, target_port);
        match TcpStream::connect(&addr).await {
            Ok(mut server) => {
                client
                    .write_all(b"HTTP/1.1 200 Connection Established\r\nProxy-agent: Echobird-Direct\r\n\r\n")
                    .await
                    .map_err(|e| format!("Write error: {}", e))?;

                let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
            }
            Err(e) => {
                log::error!("[Proxy] Direct connection failed for {}: {}", target_host, e);
                let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
            }
        }
    }

    Ok(())
}

/// Parse "host:port" string
fn parse_host_port(s: &str) -> Result<(String, u16), String> {
    if let Some(colon_pos) = s.rfind(':') {
        let host = &s[..colon_pos];
        let port: u16 = s[colon_pos + 1..]
            .parse()
            .map_err(|_| format!("Invalid port in {}", s))?;
        Ok((host.to_string(), port))
    } else {
        Ok((s.to_string(), 443))
    }
}

/// Parse SS URL (ss://...) into SSNode config
pub fn parse_ss_url(ss_url: &str) -> Option<SSNode> {
    let parsed = url::Url::parse(ss_url).ok()?;
    let port = parsed.port().unwrap_or(8388);
    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

    Some(SSNode {
        name: "SS Proxy".to_string(),
        server: parsed.host_str()?.to_string(),
        port,
        cipher: params.get("cipher").cloned().unwrap_or_else(|| "aes-256-gcm".to_string()),
        password: params.get("password").cloned().unwrap_or_default(),
    })
}

// ─── Global singleton ───

use tokio::sync::OnceCell;

static PROXY_INSTANCE: OnceCell<Mutex<ProxyServer>> = OnceCell::const_new();

/// Start proxy (singleton)
pub async fn start_proxy_server(config: Option<SSNode>) -> Result<u16, String> {
    let proxy = PROXY_INSTANCE
        .get_or_init(|| async { Mutex::new(ProxyServer::new()) })
        .await;

    let mut server = proxy.lock().await;
    if server.port() > 0 {
        return Ok(server.port());
    }
    server.start(config).await
}

/// Stop proxy
pub async fn stop_proxy_server() {
    if let Some(proxy) = PROXY_INSTANCE.get() {
        let server = proxy.lock().await;
        log::info!("[Proxy] Server stopped (port {})", server.port());
        // Note: actual listener shutdown would require storing the JoinHandle
    }
}

/// Get proxy port
pub async fn get_proxy_port() -> u16 {
    if let Some(proxy) = PROXY_INSTANCE.get() {
        proxy.lock().await.port()
    } else {
        0
    }
}

/// Add host rule to proxy
pub async fn add_host_rule(hostname: &str, ss_node: SSNode) {
    if let Some(proxy) = PROXY_INSTANCE.get() {
        proxy.lock().await.add_host_rule(hostname.to_string(), ss_node).await;
    }
}

/// Clear all host rules
pub async fn clear_host_rules() {
    if let Some(proxy) = PROXY_INSTANCE.get() {
        proxy.lock().await.clear_host_rules().await;
    }
}
