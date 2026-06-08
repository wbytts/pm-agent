import { invoke } from "@tauri-apps/api/core";

export type AgentRole = "System" | "User" | "Assistant" | "Tool";

export interface AgentMessage {
  role: AgentRole;
  content: string;
}

export interface AiModel {
  id: string;
  provider: string;
  api: string;
  displayName: string;
  contextWindow: number;
}

export interface PmAgentSession {
  id: string;
  title: string;
  messages: AgentMessage[];
  tools: Array<{ name: string; description: string; kind: string }>;
  workspaceCwd: string | null;
  model: AiModel;
}

export interface PmAgentResponse {
  session: PmAgentSession;
}

export function createAgentSession() {
  return invoke<PmAgentSession>("pm_agent_create_session");
}

export function sendAgentPrompt(session: PmAgentSession, prompt: string) {
  return invoke<PmAgentResponse>("pm_agent_send_prompt", { session, prompt });
}

export function listAgentModels() {
  return invoke<AiModel[]>("pm_agent_list_models");
}

export function setAgentSessionModel(session: PmAgentSession, model: AiModel) {
  return invoke<PmAgentSession>("pm_agent_set_session_model", { session, model });
}
