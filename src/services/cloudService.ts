import { callCommand } from "./tauri";
import type {
  CloudAuditEvent,
  CloudCreatedInvitation,
  CloudInvitation,
  CloudProvisionedMember,
  CloudRole,
  CloudRoleWithPermissions,
  CloudServer,
  CloudSessionInfo,
  CloudTeam,
  CloudTeamMember,
  CloudUserProfile,
} from "@/types/cloud";

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

/** Not called from anywhere yet, and deliberately kept.
 *
 * `cloud_config.rs`'s own doc comment says a self-hosted backend's URL is set
 * "via settings" - settings that do not exist. Deleting this pair as unused
 * code would make that documented path disappear rather than appear, leaving
 * self-hosters hand-editing a JSON file nothing tells them about. The gap is
 * the missing settings field, not this wrapper. Recorded in `FIX_PLAN.md`
 * under E.4 rather than quietly resolved in either direction. */
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

export function cloudListPermissions(): Promise<string[]> {
  return callCommand<string[]>("cloud_list_permissions");
}

export function cloudListRoles(teamId: string): Promise<CloudRoleWithPermissions[]> {
  return callCommand<CloudRoleWithPermissions[]>("cloud_list_roles", { teamId });
}

export function cloudCreateRole(
  teamId: string,
  name: string,
  description: string | null,
  permissions: string[],
): Promise<CloudRoleWithPermissions> {
  return callCommand<CloudRoleWithPermissions>("cloud_create_role", { teamId, name, description, permissions });
}

export function cloudUpdateRole(
  teamId: string,
  roleId: string,
  name: string,
  description: string | null,
  permissions: string[],
): Promise<CloudRoleWithPermissions> {
  return callCommand<CloudRoleWithPermissions>("cloud_update_role", { teamId, roleId, name, description, permissions });
}

export function cloudDeleteRole(teamId: string, roleId: string): Promise<void> {
  return callCommand<void>("cloud_delete_role", { teamId, roleId });
}

export function cloudListMemberRoles(teamId: string, userId: string): Promise<CloudRole[]> {
  return callCommand<CloudRole[]>("cloud_list_member_roles", { teamId, userId });
}

export function cloudAssignRole(teamId: string, userId: string, roleId: string): Promise<void> {
  return callCommand<void>("cloud_assign_role", { teamId, userId, roleId });
}

export function cloudUnassignRole(teamId: string, userId: string, roleId: string): Promise<void> {
  return callCommand<void>("cloud_unassign_role", { teamId, userId, roleId });
}

export function cloudListServers(teamId: string): Promise<CloudServer[]> {
  return callCommand<CloudServer[]>("cloud_list_servers", { teamId });
}

export function cloudCreateServer(
  teamId: string,
  name: string,
  host: string,
  sshPort: number,
  username: string | null,
): Promise<CloudServer> {
  return callCommand<CloudServer>("cloud_create_server", { teamId, name, host, sshPort, username });
}

export function cloudDeleteServer(teamId: string, serverId: string): Promise<void> {
  return callCommand<void>("cloud_delete_server", { teamId, serverId });
}

export function cloudMyPermissions(teamId: string): Promise<string[]> {
  return callCommand<string[]>("cloud_my_permissions", { teamId });
}

export function cloudRemoveMember(teamId: string, userId: string): Promise<void> {
  return callCommand<void>("cloud_remove_member", { teamId, userId });
}

export function cloudDeleteTeam(teamId: string): Promise<void> {
  return callCommand<void>("cloud_delete_team", { teamId });
}

export function cloudListInvitations(teamId: string): Promise<CloudInvitation[]> {
  return callCommand<CloudInvitation[]>("cloud_list_invitations", { teamId });
}

/**
 * Creates an account for somebody and adds them to the team.
 *
 * The password in the result is the only copy that will exist outside a
 * hash - show it, let it be copied, and do not try to store it.
 */
export function cloudProvisionMember(
  teamId: string,
  email: string,
  displayName: string | null,
  roleId: string | null,
): Promise<CloudProvisionedMember> {
  return callCommand<CloudProvisionedMember>("cloud_provision_member", { teamId, email, displayName, roleId });
}

/**
 * Replaces the signed-in account's own password.
 *
 * Every other session ends server-side; the Rust side adopts the session
 * that comes back, so this app stays signed in.
 */
export function cloudChangePassword(currentPassword: string, newPassword: string): Promise<CloudUserProfile> {
  return callCommand<CloudUserProfile>("cloud_change_password", { currentPassword, newPassword });
}

export function cloudCreateInvitation(
  teamId: string,
  email: string,
  roleId: string | null,
  expiresInDays: number | null,
): Promise<CloudCreatedInvitation> {
  return callCommand<CloudCreatedInvitation>("cloud_create_invitation", { teamId, email, roleId, expiresInDays });
}

export function cloudRevokeInvitation(teamId: string, invitationId: string): Promise<void> {
  return callCommand<void>("cloud_revoke_invitation", { teamId, invitationId });
}

export function cloudAcceptInvitation(token: string): Promise<CloudTeam> {
  return callCommand<CloudTeam>("cloud_accept_invitation", { token });
}

export function cloudDeclineInvitation(token: string): Promise<void> {
  return callCommand<void>("cloud_decline_invitation", { token });
}

export function cloudListAuditEvents(teamId: string, limit: number, offset: number): Promise<CloudAuditEvent[]> {
  return callCommand<CloudAuditEvent[]>("cloud_list_audit_events", { teamId, limit, offset });
}
