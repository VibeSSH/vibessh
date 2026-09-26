import { callCommand } from "./tauri";
import type { AppInfo } from "@/types/common";

export function getAppInfo(): Promise<AppInfo> {
  return callCommand<AppInfo>("get_app_info");
}

/** Why VibeSSH did not start - see the Rust side's `crash_report`. */
export interface StartupFailure {
  /** A failed command's error, as the Rust side serializes it; `normalizeError` reads it. */
  error: unknown;
  /** The report file's text. */
  report: string;
  /** Where the report was written, or null if it could not be. */
  reportPath: string | null;
}

/** Why VibeSSH did not start, or null when it did. Asked before anything renders. */
export function getStartupFailure(): Promise<StartupFailure | null> {
  return callCommand<StartupFailure | null>("get_startup_failure");
}

/** Opens the file manager on the startup failure's report file. */
export function revealCrashReport(): Promise<void> {
  return callCommand<void>("reveal_crash_report");
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
