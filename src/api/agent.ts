// Agent APIs — Mother Agent send/abort/reset/listen
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { AgentRequest, AgentEvent } from './types';

export async function sendAgentMessage(request: AgentRequest): Promise<string> {
  return invoke('agent_send_message', { request });
}

export async function abortAgent(serverKey: string): Promise<boolean> {
  return invoke('agent_abort', { serverKey });
}

export async function resetAgent(serverKey: string): Promise<string> {
  return invoke('agent_reset', { serverKey });
}

export function listenAgentEvents(handler: (event: AgentEvent) => void): Promise<UnlistenFn> {
  return listen<AgentEvent>('agent_event', (e) => handler(e.payload));
}
