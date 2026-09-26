import type { ApplicationDetail } from "@/types/application";
import { callCommand } from "./tauri";

/** Mirrors the Rust `ScheduleAction`. Power actions only, for now. */
export type ScheduleAction = "start" | "stop" | "restart";

export interface ApplicationSchedule {
  id: string;
  applicationId: string;
  name: string;
  /** Five cron fields, in the Node's own time zone. */
  cron: string;
  action: ScheduleAction;
  enabled: boolean;
  createdAt: string;
}

export interface ScheduleInput {
  name: string;
  cron: string;
  action: ScheduleAction;
  enabled: boolean;
}

/** The last run the Node recorded for a schedule. */
export interface ScheduleRun {
  scheduleId: string;
  ranAt: string;
  action: string;
  exitCode: number;
  message: string;
}

export interface NodeTimeZone {
  name: string | null;
  offsetMinutes: number;
}

export interface ApplicationSchedules {
  schedules: ApplicationSchedule[];
  lastRuns: ScheduleRun[];
  timeZone: NodeTimeZone | null;
  /** Why the Node could not be asked. The schedules themselves still load. */
  nodeError: string | null;
}

export function listApplicationSchedules(applicationId: string): Promise<ApplicationSchedules> {
  return callCommand<ApplicationSchedules>("list_application_schedules", { id: applicationId });
}

export function createApplicationSchedule(applicationId: string, input: ScheduleInput): Promise<ApplicationSchedule> {
  return callCommand<ApplicationSchedule>("create_application_schedule", { id: applicationId, input });
}

export function updateApplicationSchedule(scheduleId: string, input: ScheduleInput): Promise<ApplicationSchedule> {
  return callCommand<ApplicationSchedule>("update_application_schedule", { scheduleId, input });
}

export function deleteApplicationSchedule(scheduleId: string): Promise<void> {
  return callCommand<void>("delete_application_schedule", { scheduleId });
}

/** Runs the schedule's action now, through the same runner cron uses. */
export function runApplicationScheduleNow(scheduleId: string): Promise<void> {
  return callCommand<void>("run_application_schedule_now", { scheduleId });
}

/** What the Node's last disk check found. */
export interface DiskUsage {
  checkedAt: string;
  usedBytes: number;
  limitBytes: number;
  /** Whether that check stopped the server for being over the limit. */
  stopped: boolean;
}

/** Sets or clears the disk limit (in MB) the Node enforces. Resolves with the updated application. */
export function setApplicationDiskLimit(applicationId: string, limitMb: number | null): Promise<ApplicationDetail> {
  return callCommand<ApplicationDetail>("set_application_disk_limit", { id: applicationId, limitMb });
}

/** The Node's last disk check, or null before the first one. */
export function getApplicationDiskUsage(applicationId: string): Promise<DiskUsage | null> {
  return callCommand<DiskUsage | null>("get_application_disk_usage", { id: applicationId });
}

/** Installs cron on a Node that has none - what a `cron_missing` error offers. */
export function installCron(serverId: string): Promise<void> {
  return callCommand<void>("install_cron", { serverId });
}
