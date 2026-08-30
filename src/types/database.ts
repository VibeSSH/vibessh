/** Mirrors the Rust `DatabaseEngine` enum - both are wire-compatible (same `mysql` CLI, same SQL), so this is display-only, never a branch in how provisioning runs. */
export type DatabaseEngine = "mysql" | "mariadb";

/** A MySQL/MariaDB engine VibeSSH can provision databases on - almost always the same Server an application itself runs on. */
export interface DatabaseHost {
  id: string;
  /** `undefined` = not a VibeSSH-managed Server - still listed, but VibeSSH has no SSH connection to provision through until one is linked. */
  serverId?: string;
  name: string;
  engine: DatabaseEngine;
  /** As reachable from that host's own shell, e.g. "127.0.0.1" - not necessarily reachable from this desktop directly. */
  host: string;
  port: number;
  adminUsername: string;
  /** Set once a phpMyAdmin Application (any Docker application - see `setDatabaseHostPhpmyadmin`) is linked to this host. */
  phpmyadminApplicationId?: string;
  createdAt: string;
  updatedAt: string;
}

export interface CreateDatabaseHostInput {
  serverId?: string;
  name: string;
  engine: DatabaseEngine;
  host: string;
  port: number;
  adminUsername: string;
  adminPassword: string;
}

/** One provisioned database + scoped user for a single Application. `databaseName`/`username` are always machine-generated - there's no field to type them in. */
export interface ApplicationDatabase {
  id: string;
  applicationId: string;
  databaseHostId: string;
  databaseName: string;
  username: string;
  connectionsFrom: string;
  createdAt: string;
}
