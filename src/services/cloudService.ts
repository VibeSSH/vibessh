import { callCommand } from "./tauri";
import type { CloudSessionInfo, CloudTeam, CloudTeamMember, CloudUserProfile } from "@/types/cloud";

export function cloudRegister(email: string, password: string, displayName: string): Promise<CloudUserProfile> {
  return callCommand<CloudUserProfile>("cloud_register", { email, password, displayName });
}

export function cloudLogin(email: string, password: string): Promise<CloudUserProfile> {
  return callCommand<CloudUserProfile>("cloud_login", { email, password });
}

export function cloudLogout(): Promise<void> {
  return callCommand<void>("cloud_logout");
}

/** `null` outside a Tauri webview or when nothing is signed in - never throws for "not signed in", since that's the normal starting state for most users. */
export function cloudSessionInfo(): Promise<CloudSessionInfo | null> {
  return callCommand<CloudSessionInfo | null>("cloud_session_info").catch(() => null);
}

export function cloudGetBackendUrl(): Promise<string> {
  return callCommand<string>("cloud_get_backend_url");
}

export function cloudSetBackendUrl(backendUrl: string): Promise<void> {
  return callCommand<void>("cloud_set_backend_url", { backendUrl });
}

export function cloudListTeams(): Promise<CloudTeam[]> {
  return callCommand<CloudTeam[]>("cloud_list_teams");
}

export function cloudCreateTeam(name: string): Promise<CloudTeam> {
  return callCommand<CloudTeam>("cloud_create_team", { name });
}

export function cloudListMembers(teamId: string): Promise<CloudTeamMember[]> {
  return callCommand<CloudTeamMember[]>("cloud_list_members", { teamId });
}

export function cloudGetTeam(teamId: string): Promise<CloudTeam> {
  return callCommand<CloudTeam>("cloud_get_team", { teamId });
}
