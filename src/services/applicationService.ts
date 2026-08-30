import { callCommand } from "./tauri";
import type { Application, ApplicationDetail, ApplicationStatus, Blueprint, EnvironmentVariable, ResourceUsage, RuntimeType } from "@/types/application";

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
