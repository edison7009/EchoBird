// SSH APIs — Server persistence, encryption, and connection testing
import { invoke } from '@tauri-apps/api/core';

export interface SSHConnectResult {
    success: boolean;
    message: string;
}

export async function sshTestConnection(host: string, port: number, username: string, password: string): Promise<SSHConnectResult> {
    return invoke('ssh_test_connection', { host, port, username, password });
}

export interface SSHServer {
    id: string;
    host: string;
    port: number;
    username: string;
    password: string; // encrypted (enc:v1:...)
    alias?: string;   // user-defined display name
}

export async function loadSSHServers(): Promise<SSHServer[]> {
    return invoke('load_ssh_servers');
}

export async function saveSSHServer(id: string, host: string, port: number, username: string, password: string, alias?: string): Promise<SSHServer> {
    return invoke('save_ssh_server', { id, host, port, username, password, alias: alias ?? null });
}

export async function removeSSHServerFromDisk(id: string): Promise<boolean> {
    return invoke('remove_ssh_server', { id });
}

export async function decryptSSHPassword(encrypted: string): Promise<string> {
    return invoke('decrypt_ssh_password', { encrypted });
}

export async function encryptSSHPassword(plaintext: string): Promise<string> {
    return invoke('encrypt_ssh_password', { plaintext });
}
