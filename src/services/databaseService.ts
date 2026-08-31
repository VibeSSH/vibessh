import { callCommand } from "./tauri";
import type { ApplicationDatabase, CreateDatabaseHostInput, DatabaseHost } from "@/types/database";

export function listDatabaseHosts(): Promise<DatabaseHost[]> {
  return callCommand<DatabaseHost[]>("list_database_hosts");
}

export function createDatabaseHost(input: CreateDatabaseHostInput): Promise<DatabaseHost> {
  return callCommand<DatabaseHost>("create_database_host", { input });
}

/** Rejected if the host still has databases provisioned on it - remove those first. */
export function deleteDatabaseHost(id: string): Promise<void> {
  return callCommand<void>("delete_database_host", { id });
}

/** `applicationId: undefined` unlinks - see `DatabaseHost.phpmyadminApplicationId`'s own doc comment. */
export function setDatabaseHostPhpmyadmin(id: string, applicationId?: string): Promise<DatabaseHost> {
  return callCommand<DatabaseHost>("set_database_host_phpmyadmin", { id, applicationId });
}

export function listApplicationDatabases(applicationId: string): Promise<ApplicationDatabase[]> {
  return callCommand<ApplicationDatabase[]>("list_application_databases", { applicationId });
}

/** `purpose` seeds the generated name/username (e.g. "vibessh_myserver_a1b2c3") - everything else about them is opaque and random. Provisions for real over SSH; can take a few seconds. */
export function createApplicationDatabase(applicationId: string, databaseHostId: string, purpose?: string): Promise<ApplicationDatabase> {
  return callCommand<ApplicationDatabase>("create_application_database", { applicationId, databaseHostId, purpose });
}

export function deleteApplicationDatabase(id: string): Promise<void> {
  return callCommand<void>("delete_application_database", { id });
}

/** The stored password straight from the OS keyring - no network round trip, safe to call just to reveal it in the UI. */
export function revealApplicationDatabasePassword(id: string): Promise<string> {
  return callCommand<string>("reveal_application_database_password", { id });
}

/** Generates a new password, applies it on the actual database server over SSH, and returns it once - same "shown once" shape as everywhere else a generated secret surfaces in this app. */
export function resetApplicationDatabasePassword(id: string): Promise<string> {
  return callCommand<string>("reset_application_database_password", { id });
}

/** Rejects if no phpMyAdmin application is linked to this host, or if it has no published port - never a URL that would just fail to load. `databaseName` pre-fills phpMyAdmin's own `db=` query parameter where its configuration allows it. */
export function getPhpmyadminUrl(databaseHostId: string, databaseName?: string): Promise<string> {
  return callCommand<string>("get_phpmyadmin_url", { databaseHostId, databaseName });
}
