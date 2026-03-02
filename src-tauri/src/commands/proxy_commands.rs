// Tauri Commands for proxy server operations

use crate::models::config::ProxyRule;
use crate::models::model::SSNode;
use crate::services::proxy_server;

/// Start the proxy server
#[tauri::command]
pub async fn start_proxy(config: Option<SSNode>) -> Result<u16, String> {
    proxy_server::start_proxy_server(config).await
}

/// Stop the proxy server
#[tauri::command]
pub async fn stop_proxy() -> Result<(), String> {
    proxy_server::stop_proxy_server().await;
    Ok(())
}

/// Get proxy port
#[tauri::command]
pub async fn get_proxy_port() -> u16 {
    proxy_server::get_proxy_port().await
}

/// Get proxy rules
#[tauri::command]
pub fn get_proxy_rules() -> Vec<ProxyRule> {
    proxy_server::get_proxy_rules()
}

/// Save proxy rules
#[tauri::command]
pub fn save_proxy_rules(rules: Vec<ProxyRule>) {
    proxy_server::save_proxy_rules(&rules);
}

/// Add host rule for proxy routing
#[tauri::command]
pub async fn add_proxy_host_rule(hostname: String, ss_node: SSNode) {
    proxy_server::add_host_rule(&hostname, ss_node).await;
}

/// Clear all proxy host rules
#[tauri::command]
pub async fn clear_proxy_host_rules() {
    proxy_server::clear_host_rules().await;
}

/// Parse SS URL to config
#[tauri::command]
pub fn parse_ss_url(url: String) -> Option<SSNode> {
    proxy_server::parse_ss_url(&url)
}
