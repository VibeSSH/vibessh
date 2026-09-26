import { callCommand } from "./tauri";
import type {
  CloudApplicationMember,
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

/** The second factor, sent again with the email and password after the first attempt came back `two_factor_required`. */
export interface SecondFactor {
  totpCode?: string;
  recoveryCode?: string;
}

export function cloudLogin(email: string, password: string, secondFactor?: SecondFactor): Promise<CloudUserProfile> {
  return callCommand<CloudUserProfile>("cloud_login", { email, password, totpCode: secondFactor?.totpCode ?? null, recoveryCode: secondFactor?.recoveryCode ?? null });
}

/** A two-factor setup in progress - the QR code is drawn on this computer. */
export interface TwoFactorSetup {
  secret: string;
  otpauthUri: string;
  qrSvg: string;
}

export function cloudTwoFactorSetup(): Promise<TwoFactorSetup> {
  return callCommand<TwoFactorSetup>("cloud_two_factor_setup");
}

/** Turns two-factor on; resolves with the recovery codes, shown this once. */
export function cloudTwoFactorEnable(code: string): Promise<{ recoveryCodes: string[] }> {
  return callCommand<{ recoveryCodes: string[] }>("cloud_two_factor_enable", { code });
}

export function cloudTwoFactorDisable(password: string, secondFactor: SecondFactor): Promise<CloudUserProfile> {
  return callCommand<CloudUserProfile>("cloud_two_factor_disable", { password, totpCode: secondFactor.totpCode ?? null, recoveryCode: secondFactor.recoveryCode ?? null });
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
  /** Permissions granted on one application that no sudo rule could carry. */
  notes: SkippedGrant[];
}

/** Why a per-application permission was not written - see the Rust side's `member_sudoers::SkipReason`. */
export interface SkippedGrant {
  applicationId: string;
  reason: "folder_unnameable" | "no_console" | "nothing_to_name";
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
 * This removes it from the team's view. The key comes off the Nodes it was
 * already installed on at the next `syncTeamNodeAccess` for each of them,
 * run from an install that can reach the machine - because that sync writes
 * `authorized_keys` whole, from the keys still published. Until one runs,
 * the key is still there, and anything offering this has to say so rather
 * than implying the access is already gone.
 */
export function cloudRevokeDevice(keyId: string): Promise<void> {
  return callCommand<void>("cloud_revoke_device", { keyId });
}

/**
 * What happened to one person's access that the team has taken away.
 *
 * `completed` is what the Node did, not what was asked for: a revocation
 * that failed stays pending here and stays pending on the screen, because
 * their key is still in a file on that machine.
 */
export interface RevocationResult {
  id: string;
  email: string;
  nodeUsername: string;
  completed: boolean;
  error: string | null;
}

/** Everything one sync did to one Node. */
export interface NodeAccessSync {
  members: MemberAccessResult[];
  revocations: RevocationResult[];
}

/**
 * What this person logs in as on a shared Node, and with which key.
 *
 * Both were already decided and neither was anywhere they could see it - a
 * teammate was given a real account on a real machine with no way to learn
 * its name, which is why the feature looked broken while working exactly as
 * designed.
 */
export interface MyNodeAccess {
  nodeUsername: string;
  privateKeyPath: string;
  /** False until somebody has run a sync on the Node. */
  published: boolean;
}

export function cloudMyNodeAccess(teamId: string): Promise<MyNodeAccess> {
  return callCommand<MyNodeAccess>("cloud_my_node_access", { teamId });
}

/** Access removed in the team that is still on a Node. */
export interface NodeRevocation {
  id: string;
  teamServerId: string;
  serverName: string;
  host: string;
  sshPort: number;
  userId: string;
  nodeUsername: string;
  email: string;
  requestedAt: string;
}

/**
 * Makes one Node hold exactly the access the team describes.
 *
 * Every current member's account and currently published keys - written
 * whole, so a device somebody revoked stops working here too - and then
 * every revocation the team is still owed on that machine.
 */
export function syncTeamNodeAccess(serverId: string, teamId: string, teamServerId: string): Promise<NodeAccessSync> {
  return callCommand<NodeAccessSync>("sync_team_node_access", { serverId, teamId, teamServerId });
}

/**
 * What the team has asked to be taken off its Nodes and has not been.
 *
 * Read from the backend rather than remembered from the last sync: the
 * install that removed somebody is often not the one that can reach the
 * machine, and this list is the only thing that carries the fact between
 * them.
 */
export function listPendingRevocations(teamId: string): Promise<NodeRevocation[]> {
  return callCommand<NodeRevocation[]>("cloud_list_pending_revocations", { teamId });
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

/**
 * Who, of a team's members, may see one shared application.
 *
 * An empty list is not "nobody" - a shared application with no members listed
 * is visible to the whole team, and adding the first member is what restricts
 * it to exactly the people listed here (plus whoever shared it). The backend
 * enforces that; see `apps/backend/src/team_applications.rs`.
 */
export function listApplicationMembers(teamId: string, applicationId: string): Promise<CloudApplicationMember[]> {
  return callCommand<CloudApplicationMember[]>("list_application_members", { teamId, applicationId });
}

export function addApplicationMember(teamId: string, applicationId: string, userId: string): Promise<void> {
  return callCommand<void>("add_application_member", { teamId, applicationId, userId });
}

/** What one sync of shared applications did - see `syncSharedApplications`. */
export interface SharedSyncReport {
  added: number;
  refreshed: number;
  removed: number;
}

/** One of this install's applications that is somebody else's, shared with this account. */
export interface SharedApplicationAccess {
  applicationId: string;
  teamId: string;
  permissions: string[];
}

/**
 * Puts the applications teammates shared with this account on this install,
 * and takes off the ones no longer shared - only on Nodes this install
 * connects to as its own member account. The row only: a container is its
 * owner's and is never touched from here.
 */
export function syncSharedApplications(): Promise<SharedSyncReport> {
  return callCommand<SharedSyncReport>("sync_shared_applications");
}

export function listSharedApplicationAccess(): Promise<SharedApplicationAccess[]> {
  return callCommand<SharedApplicationAccess[]>("list_shared_application_access");
}

/** Replaces what one member may do with a shared application they can see. */
export function setApplicationMemberPermissions(teamId: string, applicationId: string, userId: string, permissions: string[]): Promise<void> {
  return callCommand<void>("set_application_member_permissions", { teamId, applicationId, userId, permissions });
}

export function removeApplicationMember(teamId: string, applicationId: string, userId: string): Promise<void> {
  return callCommand<void>("remove_application_member", { teamId, applicationId, userId });
}
