import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { callCommand } from "./tauri";
import type {
  Application,
  ApplicationDetail,
  ApplicationPort,
  ApplicationStatus,
  Blueprint,
  EnvironmentVariable,
  HealthStatus,
  JavaInstallation,
  PortInput,
  RegistryCredential,
  ResourceUsage,
  RuntimeType,
  SetHealthCheckInput,
  SetRegistryCredentialInput,
  SetResourceLimitsInput,
} from "@/types/application";

/** What the Create Application wizard submits - mirrors the Rust `CreateApplicationFromBlueprintInput` DTO. */
export interface CreateApplicationInput {
  serverId?: string;
  name: string;
  description?: string;
  blueprintId: string;
  runtimeType: RuntimeType;
  workingDirectory: string;
  environment: EnvironmentVariable[];
  /** `{ fieldKey: value }` - keys matching the chosen blueprint's own field keys. */
  blueprintInputs: Record<string, unknown>;
  /**
   * The Application this one should be pointed at, for a blueprint that
   * declares a `connectsTo` (phpMyAdmin at a MariaDB). The backend derives
   * both the environment rows naming it and the connection that makes it
   * reachable - the frontend only says which one.
   */
  connectToApplicationId?: string;
}

export function listApplications(): Promise<Application[]> {
  return callCommand<Application[]>("list_applications");
}

export function getApplication(id: string): Promise<ApplicationDetail> {
  return callCommand<ApplicationDetail>("get_application", { id });
}

export function listBlueprints(): Promise<Blueprint[]> {
  return callCommand<Blueprint[]>("list_blueprints");
}

export function createApplication(input: CreateApplicationInput): Promise<ApplicationDetail> {
  return callCommand<ApplicationDetail>("create_application", { input });
}

/**
 * What a delete actually managed to clean up. Deleting an Application is a
 * real teardown - it destroys the container, drops its databases, revokes
 * its firewall rules, removes its DNS name and removes its Node-side
 * account - and any of those can fail if the Node is unreachable partway
 * through. `warnings` being non-empty means the row is gone but something
 * on the Node is not: most importantly the container may still be running
 * and still holding its published port, which is what later makes a
 * replacement Application fail to start with a raw "port is already
 * allocated". Surface it rather than reporting a clean success.
 */
export interface ApplicationTeardownReport {
  containerRemoved: boolean;
  databasesDropped: number;
  firewallSynced: boolean;
  dnsSynced: boolean;
  dedicatedAccountRemoved: boolean;
  workingDirectoryRemoved: boolean;
  warnings: string[];
}

/** `removeFiles` deletes the Application's working directory - a world save, a database volume, whatever the operator put there. Off unless explicitly asked: it is the one step that cannot be undone. */
export function deleteApplication(id: string, removeFiles = false): Promise<ApplicationTeardownReport> {
  return callCommand<ApplicationTeardownReport>("delete_application", { id, removeFiles });
}

/** Re-renders `runtimeConfig` from the blueprint after merging `fieldValues` on top of whatever was stored at creation (or the last edit) - see the Rust `update_application_config`'s own doc comment. A restart is required for a running process to actually pick up the new config, same as an uploaded jar replacement. */
/**
 * Renames an Application.
 *
 * The name is also the hostname other Applications resolve this one by on
 * their shared private network, and that follows the new name only from the
 * next restart - see the Rust side for why.
 */
export function renameApplication(id: string, name: string): Promise<ApplicationDetail> {
  return callCommand<ApplicationDetail>("rename_application", { id, name });
}

export function updateApplicationConfig(id: string, fieldValues: Record<string, unknown>): Promise<ApplicationDetail> {
  return callCommand<ApplicationDetail>("update_application_config", { id, fieldValues });
}

/**
 * Moves an Application to a different blueprint - taking version management
 * over, or giving it up.
 *
 * `fieldValues` are the new blueprint's own answers; the old blueprint's are
 * discarded rather than carried across, since nothing reads a `generic-docker`
 * image off a Paper application. Going to a managed blueprint provisions,
 * which downloads that server's jar into the working directory - see
 * `BlueprintSwitchCard` for the warning that belongs to that direction.
 */
export function changeApplicationBlueprint(
  id: string,
  blueprintId: string,
  fieldValues: Record<string, unknown>,
): Promise<ApplicationDetail> {
  return callCommand<ApplicationDetail>("change_application_blueprint", { id, blueprintId, fieldValues });
}

export function startApplication(id: string): Promise<ApplicationStatus> {
  return callCommand<ApplicationStatus>("start_application", { id });
}

/** `graceful = true` waits for the application to actually stop before resolving; `false` fires the stop signal and returns immediately - see the matching Rust runtimes' own doc comments for what "graceful" means per runtime type. */
export function stopApplication(id: string, graceful: boolean): Promise<ApplicationStatus> {
  return callCommand<ApplicationStatus>("stop_application", { id, graceful });
}

export function restartApplication(id: string): Promise<ApplicationStatus> {
  return callCommand<ApplicationStatus>("restart_application", { id });
}

/** "Recreate Container" (Etap M1, Docker applications only) - tears the container down and creates it again from the application's current config, so an edited image/command/resource limit/restart policy actually takes effect. */
export function recreateApplication(id: string): Promise<ApplicationStatus> {
  return callCommand<ApplicationStatus>("recreate_application", { id });
}

export function killApplication(id: string): Promise<ApplicationStatus> {
  return callCommand<ApplicationStatus>("kill_application", { id });
}

export function refreshApplicationStatus(id: string): Promise<ApplicationStatus> {
  return callCommand<ApplicationStatus>("refresh_application_status", { id });
}

export function getApplicationResourceUsage(id: string): Promise<ResourceUsage> {
  return callCommand<ResourceUsage>("get_application_resource_usage", { id });
}

/** A snapshot, not a live stream - same pull-on-demand shape as the existing container logs panel; there's no live console/log-streaming infrastructure yet (see runtime::mod's own LogProvider doc comment). */
export function getApplicationLogs(id: string, maxLines: number): Promise<string[]> {
  return callCommand<string[]>("get_application_logs", { id, maxLines });
}

/** Sends one line to the application's stdin/console - rejects with a clear message when the runtime has no console at all, or reports it as read-only (see `runtime::ApplicationConsole`'s own doc comment for both cases). */
/**
 * Sends one command to the server inside an application and returns what it
 * said - the Redis/MongoDB console.
 *
 * Not `writeApplicationConsole`, which types into a process's stdin: a
 * database ignores stdin entirely. Each call is its own run, so nothing
 * carries over between commands.
 */
export function runApplicationCommand(id: string, command: string): Promise<string> {
  return callCommand<string>("run_application_command", { id, command });
}

export function writeApplicationConsole(id: string, input: string): Promise<void> {
  return callCommand<void>("write_application_console", { id, input });
}

/** Real, actually-installed Java runtimes - `serverId` undefined detects on this machine, set detects on that Remote server over SSH. Best-effort: an empty array just means the wizard's free-text fallback stays available, not an error. */
export function detectJavaInstallations(serverId?: string): Promise<JavaInstallation[]> {
  return callCommand<JavaInstallation[]>("detect_java_installations", { serverId });
}

/** Every currently-available Paper version (from papermc.io), newest first - the same list `PaperBlueprint`'s own provisioning step picks a build from. */
export function listPaperVersions(): Promise<string[]> {
  return callCommand<string[]>("list_paper_versions");
}

/** Same idea as listPaperVersions, for the Velocity proxy - a different papermc.io project, so a separate list. */
export function listVelocityVersions(): Promise<string[]> {
  return callCommand<string[]>("list_velocity_versions");
}

/** Same idea as listPaperVersions, for the Waterfall proxy - a different papermc.io project, so a separate list. */
export function listWaterfallVersions(): Promise<string[]> {
  return callCommand<string[]>("list_waterfall_versions");
}

/** Every currently-available Purpur version (from purpurmc.org, a separate build API from papermc.io), newest first. */
export function listPurpurVersions(): Promise<string[]> {
  return callCommand<string[]>("list_purpur_versions");
}

/** Declared ports are documentation of intent, not a live guarantee - VibeSSH checks for a collision against this same application's *other* declared ports, not whether the port is actually free on the host. */
export function listApplicationPorts(id: string): Promise<ApplicationPort[]> {
  return callCommand<ApplicationPort[]>("list_application_ports", { id });
}

export function addApplicationPort(id: string, port: PortInput): Promise<ApplicationPort> {
  return callCommand<ApplicationPort>("add_application_port", { id, port });
}

export function updateApplicationPort(id: string, portId: string, port: PortInput): Promise<ApplicationPort> {
  return callCommand<ApplicationPort>("update_application_port", { id, portId, port });
}

/** Mirrors the Rust `FirewallSyncResult` DTO. `backend: null` means no supported firewall was detected on this application's Node (not an error). `rulesRemoved` counts rules this same sync just revoked (a port that's been unpublished, or belonged to an Application that's been deleted/migrated away) - see the Rust `firewall` module's own doc comment for the "only ever removes a rule it can prove it added itself" safety property behind that. Never enables enforcement itself, that stays a separate, explicit action. `unenforced` is `true` when nothing is actually restricting these ports - either no backend at all, or one that's installed but switched off. It must be surfaced as a warning: a sync that reports success while leaving ports open is exactly what made "Vibe Network only" ports publicly reachable. */
export interface FirewallSyncResult {
  backend: string | null;
  active: boolean;
  rulesApplied: number;
  rulesRemoved: number;
  unenforced: boolean;
}

/** "Sync Firewall" (Etap M2, Ports tab) - re-applies the current desired rule set for this application's Node. `null` for a Local application (nothing to sync). Also fires automatically, best-effort, after every `addApplicationPort`/`updateApplicationPort` - this is for a port declared before the feature existed, or retrying after a failed sync. */
export function syncApplicationNodeFirewall(id: string): Promise<FirewallSyncResult | null> {
  return callCommand<FirewallSyncResult | null>("sync_application_node_firewall", { id });
}

/** The other applications this one is allowed to reach over its node's
 * internal Docker networking. Ids, resolved against `listApplications` by the
 * caller - which needs that list anyway, to offer the ones not yet connected.
 *
 * Reachability is default-deny: an application that has never been connected
 * to anything cannot open a socket to any other application on the node, not
 * even to a port that application declared but never published. It used to be
 * the opposite, silently - see `ConnectionsCard`'s own doc comment. */
export function listApplicationLinks(id: string): Promise<string[]> {
  return callCommand<string[]>("list_application_links", { id });
}

/** Lets two Docker applications on the same node reach each other's ports.
 *
 * Symmetric: this is implemented as a private Docker network shared by
 * exactly those two containers, and a bridge network has no direction. Takes
 * effect immediately on running containers - no restart, no recreate. */
export function connectApplications(id: string, peerId: string): Promise<void> {
  return callCommand<void>("connect_applications", { id, peerId });
}

export function disconnectApplications(id: string, peerId: string): Promise<void> {
  return callCommand<void>("disconnect_applications", { id, peerId });
}

export function removeApplicationPort(id: string, portId: string): Promise<void> {
  return callCommand<void>("remove_application_port", { id, portId });
}

/** Runs the actual probe right now (a TCP/HTTP dial or a Minecraft Server List Ping, depending on how the application's health check is configured) - not a cached value, same "pull, not push" shape as `getApplicationResourceUsage`. */
export function getApplicationHealth(id: string): Promise<HealthStatus> {
  return callCommand<HealthStatus>("get_application_health", { id });
}

export function setApplicationHealthCheck(id: string, input: SetHealthCheckInput): Promise<ApplicationDetail> {
  return callCommand<ApplicationDetail>("set_application_health_check", { id, input });
}

/** Only accepted for a Docker/systemd application - see the Rust `set_application_resource_limits`'s own doc comment for why a Local/Remote process application rejects this outright instead of silently accepting and ignoring it. */
export function setApplicationResourceLimits(id: string, input: SetResourceLimitsInput): Promise<ApplicationDetail> {
  return callCommand<ApplicationDetail>("set_application_resource_limits", { id, input });
}

/** Replaces the whole environment variable set - every runtime type accepts this (unlike resource limits). Key/value validation happens when the runtime next actually starts, not here. */
export function setApplicationEnvironment(id: string, environment: EnvironmentVariable[]): Promise<ApplicationDetail> {
  return callCommand<ApplicationDetail>("set_application_environment", { id, environment });
}

/** Docker-only, same restriction as `setApplicationResourceLimits`. Patches `runtimeConfig.image` - doesn't touch an already-running container by itself, call `recreateApplication` afterward for a running Docker application to actually pick it up. */
export function setApplicationImage(id: string, image: string): Promise<ApplicationDetail> {
  return callCommand<ApplicationDetail>("set_application_image", { id, image });
}

/** `docker pull` for this Docker application's currently-configured image, on its own Node - re-fetches whatever layers changed upstream (meaningful for a floating tag like `:latest`). Returns Docker's own pull output as proof something real happened. Same as `setApplicationImage`: doesn't affect an already-running container, `recreateApplication` still needed for that. */
export function pullApplicationImage(id: string): Promise<string> {
  return callCommand<string>("pull_application_image", { id });
}

/** Every stored private-registry login, across every registry host - shared by any Application whose image comes from one of them. */
export function listRegistryCredentials(): Promise<RegistryCredential[]> {
  return callCommand<RegistryCredential[]>("list_registry_credentials");
}

/** Sets (creating or replacing) the login for one registry host, keyed by `input.registry` - re-saving a host's credential overwrites the existing one rather than adding a duplicate. */
export function setRegistryCredential(input: SetRegistryCredentialInput): Promise<RegistryCredential> {
  return callCommand<RegistryCredential>("set_registry_credential", { input });
}

export function removeRegistryCredential(id: string): Promise<void> {
  return callCommand<void>("remove_registry_credential", { id });
}

/** Mirrors the Rust `MigrationResult` DTO. `warnings` being non-empty means
 * the migration itself succeeded - the new application exists with the old
 * one's data - but a step after the point of no return didn't: the DNS name
 * may still resolve to the old instance, the old container may still be
 * running and holding its port, or a node's firewall may be out of date.
 * `started` false means the application was migrated but isn't running. */
export interface MigrationResult {
  application: ApplicationDetail;
  filesCopied: number;
  dnsRepointed: boolean;
  started: boolean;
  warnings: string[];
}

/** "Migrate to another Node" - provisions an identical Docker application on `targetServerId`, copies its working directory over, cuts its DNS alias (if it has one) to the new instance, then retires the old one. A single blocking call - see the Rust `services::migration_service`'s own doc comment for the full step order. Docker-only; rejected server-side for any other runtime type. */
export function migrateApplication(id: string, targetServerId: string): Promise<MigrationResult> {
  return callCommand<MigrationResult>("migrate_application", { id, targetServerId });
}

/**
 * Opens a live output stream for an Application.
 *
 * Docker over SSH only - `docker logs -f` is a real follow the Node runs
 * for us, and nothing equivalent exists for a local process after the fact.
 * Anything else rejects, and the caller keeps polling, which is exactly
 * what the console did before this existed.
 *
 * `followId` is chosen by the caller so it can subscribe to the events
 * before the stream starts and cannot miss the first lines.
 */
export function followApplicationLogs(applicationId: string, followId: string, tail: number): Promise<void> {
  return callCommand<void>("follow_application_logs", { id: applicationId, followId, tail });
}

/**
 * Stops the console stream on an Application.
 *
 * Takes the Application, not the follow id: the backend keeps one stream per
 * Application, so a caller that has lost track of which follow it started -
 * a remount, a reconnect - can still end it. Not optional bookkeeping; this
 * is what closes the SSH channel, and `sshd` allows ten per connection.
 */
export function stopFollowingApplicationLogs(applicationId: string): Promise<boolean> {
  return callCommand<boolean>("stop_following_application_logs", { id: applicationId });
}

/** Same not-in-a-Tauri-webview guard as the terminal helpers. */
export function onApplicationLogLine(followId: string, handler: (line: string) => void): Promise<UnlistenFn> {
  return listen<string>(`applog://${followId}/line`, (event) => handler(event.payload)).catch(() => () => {});
}

export function onApplicationLogClosed(followId: string, handler: (reason: string | null) => void): Promise<UnlistenFn> {
  return listen<string | null>(`applog://${followId}/closed`, (event) => handler(event.payload)).catch(() => () => {});
}
