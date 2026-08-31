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
  ResourceUsage,
  RuntimeType,
  SetHealthCheckInput,
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

export function deleteApplication(id: string): Promise<void> {
  return callCommand<void>("delete_application", { id });
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

/** Mirrors the Rust `FirewallSyncResult` DTO. `backend: null` means no supported firewall was detected on this application's Node (not an error) - see the Rust `firewall` module's own doc comment for why this only ever adds rules, never removes or enables enforcement. */
export interface FirewallSyncResult {
  backend: string | null;
  active: boolean;
  rulesApplied: number;
}

/** "Sync Firewall" (Etap M2, Ports tab) - re-applies the current desired rule set for this application's Node. `null` for a Local application (nothing to sync). Also fires automatically, best-effort, after every `addApplicationPort`/`updateApplicationPort` - this is for a port declared before the feature existed, or retrying after a failed sync. */
export function syncApplicationNodeFirewall(id: string): Promise<FirewallSyncResult | null> {
  return callCommand<FirewallSyncResult | null>("sync_application_node_firewall", { id });
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

/** Mirrors the Rust `MigrationResult` DTO. */
export interface MigrationResult {
  application: ApplicationDetail;
  filesCopied: number;
  dnsRepointed: boolean;
}

/** "Migrate to another Node" - provisions an identical Docker application on `targetServerId`, copies its working directory over, cuts its DNS alias (if it has one) to the new instance, then retires the old one. A single blocking call - see the Rust `services::migration_service`'s own doc comment for the full step order. Docker-only; rejected server-side for any other runtime type. */
export function migrateApplication(id: string, targetServerId: string): Promise<MigrationResult> {
  return callCommand<MigrationResult>("migrate_application", { id, targetServerId });
}
