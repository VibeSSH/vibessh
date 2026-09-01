import { callCommand } from "./tauri";
import type { ApplicationBackup, BackupDestinationConfig, BackupSchedule, SetBackupDestinationInput } from "@/types/application";

export function listApplicationBackups(id: string): Promise<ApplicationBackup[]> {
  return callCommand<ApplicationBackup[]>("list_application_backups", { id });
}

export function createApplicationBackup(id: string): Promise<ApplicationBackup> {
  return callCommand<ApplicationBackup>("create_application_backup", { id });
}

export function deleteApplicationBackup(id: string, backupId: string): Promise<void> {
  return callCommand<void>("delete_application_backup", { id, backupId });
}

/** Requires the application to already be stopped - see the Rust `restore_backup`'s own doc comment. Resolves to the number of files written. */
export function restoreApplicationBackup(id: string, backupId: string): Promise<number> {
  return callCommand<number>("restore_application_backup", { id, backupId });
}

/** `{enabled: false, intervalHours: 24, retentionCount: 5}` when nothing has ever been configured - not an error, see the Rust `get_backup_schedule`'s own doc comment. */
export function getApplicationBackupSchedule(id: string): Promise<BackupSchedule> {
  return callCommand<BackupSchedule>("get_application_backup_schedule", { id });
}

export function setApplicationBackupSchedule(id: string, schedule: BackupSchedule): Promise<BackupSchedule> {
  return callCommand<BackupSchedule>("set_application_backup_schedule", { id, input: schedule });
}

/** Checks every Application with an enabled schedule and backs up whichever are due - meant to be called on a timer from a single, always-mounted place (see `useBackupScheduler`), never per-screen. Resolves to how many backups were actually created. */
export function runDueApplicationBackups(): Promise<number> {
  return callCommand<number>("run_due_application_backups");
}

/** The one, global S3-compatible backup destination every Application's backups can additionally upload to - see the Rust `get_backup_destination` doc comment. Never includes the secret access key. */
export function getBackupDestination(): Promise<BackupDestinationConfig> {
  return callCommand<BackupDestinationConfig>("get_backup_destination");
}

/** `secretAccessKey` blank keeps the currently stored secret - see the Rust `set_backup_destination` doc comment. Rejects if `enabled: true` and no secret is stored or supplied. */
export function setBackupDestination(input: SetBackupDestinationInput): Promise<BackupDestinationConfig> {
  return callCommand<BackupDestinationConfig>("set_backup_destination", { input });
}

/** A small upload-then-delete round trip against the currently saved destination - lets the Settings page confirm the endpoint/bucket/credentials actually work before relying on it. */
export function testBackupDestination(): Promise<void> {
  return callCommand<void>("test_backup_destination");
}
