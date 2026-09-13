import { callCommand } from "./tauri";
import type {
  CloudAuditEvent,
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

/**
 * Whether an account backend has been chosen at all.
 *
 * Asked rather than compared against a literal here: the default lives in
 * `storage/cloud_config.rs`, and a copy of it in TypeScript would go quietly
 * wrong the day it becomes a real hosted address. This way changing it is one
 * line in one file.
 */
export function cloudBackendIsConfigured(): Promise<boolean> {
  return callCommand<boolean>("cloud_backend_is_configured");
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

export function cloudListAuditEvents(teamId: string, limit: number, offset: number): Promise<CloudAuditEvent[]> {
  return callCommand<CloudAuditEvent[]>("cloud_list_audit_events", { teamId, limit, offset });
}

/** One of this account's registered devices. Mirrors the Rust `CloudDeviceKey`. */
export interface CloudDeviceKey {
  id: string;
  publicKey: string;
  label: string;
  createdAt: string;
}

/**
 * What happened for one member when access to a Node was granted.
 *
 * Per member rather than one verdict for the team: four of five working is
 * neither a success nor a failure, and the person reading needs to know
 * which one did not.
 */
export interface MemberAccessResult {
  userId: string;
  email: string;
  nodeUsername: string;
  /** `false` when that person has not opened VibeSSH on any device yet, so there is no key to install. Not an error. */
  hasKey: boolean;
  granted: boolean;
  error: string | null;
}

/**
 * Registers this device's public key so a teammate's install can put it in
 * the account it creates for this person on a shared Node.
 *
 * Safe to call whenever a session is established: the backend treats the
 * same key twice as the same device.
 */
export function cloudPublishThisDevice(): Promise<void> {
  return callCommand<void>("cloud_publish_this_device");
}

export function cloudListDevices(): Promise<CloudDeviceKey[]> {
  return callCommand<CloudDeviceKey[]>("cloud_list_devices");
}

/**
 * Forgets a device.
 *
 * This removes it from the team's view. It does **not** remove the key from
 * Nodes it was already installed on - that needs an install that can reach
 * each Node, and anything offering this has to say so rather than implying
 * the access is gone.
 */
export function cloudRevokeDevice(keyId: string): Promise<void> {
  return callCommand<void>("cloud_revoke_device", { keyId });
}

/** Gives every member of a team their own account on this Node. */
export function grantTeamNodeAccess(serverId: string, teamId: string): Promise<MemberAccessResult[]> {
  return callCommand<MemberAccessResult[]>("grant_team_node_access", { serverId, teamId });
}

/** Takes one member's access to this Node away. */
export function revokeTeamNodeAccess(serverId: string, nodeUsername: string): Promise<void> {
  return callCommand<void>("revoke_team_node_access", { serverId, nodeUsername });
}

/** One port of an Application as the team sees it. */
export interface CloudApplicationPort {
  name: string;
  protocol: string;
  internalPort: number;
  externalPort: number | null;
  visibility: string;
}

/**
 * One environment variable as the team sees it.
 *
 * A secret one arrives with an empty `value` and `isSecret` set. It is listed
 * rather than hidden on purpose: an omitted `MYSQL_ROOT_PASSWORD` reads as
 * "not configured", which would send somebody to set one that already exists.
 * The value itself never leaves the Node.
 */
export interface CloudApplicationEnvironment {
  key: string;
  value: string;
  isSecret: boolean;
}

/**
 * An Application a team can see.
 *
 * A projection of the install that owns it, refreshed by pushing again -
 * never the record a runtime acts on. `updatedAt` is when that snapshot was
 * last taken, which is the only way to tell a current one from a stale one.
 */
export interface CloudApplication {
  id: string;
  teamId: string;
  teamServerId: string | null;
  localId: string;
  name: string;
  blueprintId: string;
  runtimeType: string;
  workingDirectory: string;
  ports: CloudApplicationPort[];
  environment: CloudApplicationEnvironment[];
  updatedAt: string;
}

/** Publishes one Application so the rest of the team can see it. */
export function shareApplicationWithTeam(teamId: string, applicationId: string, teamServerId: string | null): Promise<CloudApplication> {
  return callCommand<CloudApplication>("share_application_with_team", { teamId, applicationId, teamServerId });
}

export function listTeamApplications(teamId: string): Promise<CloudApplication[]> {
  return callCommand<CloudApplication[]>("list_team_applications", { teamId });
}

/**
 * Stops sharing.
 *
 * The Application keeps running and the install that owns it is untouched -
 * only the team's copy goes. Anything offering this has to say so, because
 * "remove" next to an application name reads like deletion.
 */
export function unshareApplicationFromTeam(teamId: string, applicationId: string): Promise<void> {
  return callCommand<void>("unshare_application_from_team", { teamId, applicationId });
}
