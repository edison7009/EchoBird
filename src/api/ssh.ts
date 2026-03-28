// SSH APIs — Connection, execution, server persistence, encryption
import { invoke } from '@tauri-apps/api/core';


export interface SSHConnectResult {
    success: boolean;
    message: string;
}

export interface SSHExecResult {
    stdout: string;
    stderr: string;
    exitCode: number;
    success: boolean;
}

export async function sshConnect(id: string, host: string, port: number, username: string, password: string): Promise<SSHConnectResult> {
    return invoke('ssh_connect', { id, host, port, username, password });
}

export async function sshExecute(id: string, command: string): Promise<SSHExecResult> {
    return invoke('ssh_execute', { id, command });
}

export async function sshDisconnect(id: string): Promise<boolean> {
    return invoke('ssh_disconnect', { id });
}

export async function sshTestConnection(host: string, port: number, username: string, password: string): Promise<SSHConnectResult> {
    return invoke('ssh_test_connection', { host, port, username, password });
}

// SSH server persistence (encrypted storage)
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

export async function updateSSHAlias(id: string, alias: string): Promise<boolean> {
    return invoke('update_ssh_alias', { id, alias });
}

export async function decryptSSHPassword(encrypted: string): Promise<string> {
    return invoke('decrypt_ssh_password', { encrypted });
}

export async function encryptSSHPassword(plaintext: string): Promise<string> {
    return invoke('encrypt_ssh_password', { plaintext });
}

export async function sshUploadFile(id: string, localPath: string, remotePath: string): Promise<SSHExecResult> {
    return invoke('ssh_upload_file', { id, localPath, remotePath });
}
