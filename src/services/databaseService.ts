import { callCommand } from "./tauri";
import type { ApplicationDatabase, CreateDatabaseHostInput, DatabaseHost, UpdateDatabaseHostInput } from "@/types/database";

export function listDatabaseHosts(): Promise<DatabaseHost[]> {
  return callCommand<DatabaseHost[]>("list_database_hosts");
}

/** A blank `adminPassword` keeps the stored one. */
export function updateDatabaseHost(id: string, input: UpdateDatabaseHostInput): Promise<DatabaseHost> {
  return callCommand<DatabaseHost>("update_database_host", { id, input });
}

export function createDatabaseHost(input: CreateDatabaseHostInput): Promise<DatabaseHost> {
  return callCommand<DatabaseHost>("create_database_host", { input });
}

/** Rejected if the host still has databases provisioned on it - remove those first. */
export function deleteDatabaseHost(id: string): Promise<void> {
  return callCommand<void>("delete_database_host", { id });
}

/** `applicationId: undefined` unlinks - see `DatabaseHost.phpmyadminApplicationId`'s own doc comment. */
/** Installs MariaDB on the node behind a loopback database host, and grants
 * that host's configured admin user the privileges VibeSSH needs.
 *
 * Explicit on purpose. This used to happen by itself the first time anyone
 * created a database on such a host - an apt install, an enabled service and
 * a new superuser appearing on someone's machine as a side effect of a
 * different request, with every error along the way discarded. Offered by the
 * UI only when an operation comes back with `database_server_unavailable`. */
export function installDatabaseServer(id: string): Promise<void> {
  return callCommand<void>("install_database_server", { id });
}

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
