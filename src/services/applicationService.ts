import { callCommand } from "./tauri";
import type { Application, ApplicationDetail, ApplicationStatus, Blueprint, EnvironmentVariable, JavaInstallation, ResourceUsage, RuntimeType } from "@/types/application";

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
