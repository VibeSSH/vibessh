import { callCommand } from "./tauri";
import type { AppInfo } from "@/types/common";

export function getAppInfo(): Promise<AppInfo> {
  return callCommand<AppInfo>("get_app_info");
}

/**
 * Whether this machine can run containers.
 *
 * Asked by the wizard before it lets somebody choose the Docker runtime for a
 * local application - the alternative is finding out on the first start, with
 * an Application already created that cannot run. Talks to the daemon rather
 * than just looking for the binary: a Docker Desktop that is installed but
 * not started answers `docker --version` and nothing else.
 */
export function localDockerAvailable(): Promise<boolean> {
  return callCommand<boolean>("local_docker_available");
}

/**
 * Where a local application's files should go by default.
 *
 * Resolved by the app rather than assembled here: only the Rust side knows
 * the platform's data directory, and it comes back with the right separator
 * already in it.
 */
export function localApplicationsRoot(): Promise<string> {
  return callCommand<string>("local_applications_root");
}
