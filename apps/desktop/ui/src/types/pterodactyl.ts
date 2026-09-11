/**
 * Mirrors `services::pterodactyl_import_service`'s plan types.
 *
 * There is deliberately no API key anywhere in this file. The key crosses
 * into Rust once, when the panel is connected, and lives in the OS keyring
 * afterwards - the frontend is only ever told whether one is stored.
 */

/**
 * One thing the plan says, in a form this app can say in the reader's own
 * language: `code` names a translation key and `params` carries the
 * specifics. Rust never assembles a finished sentence - see
 * `pterodactyl::mapping`'s own doc comment for why.
 */
export interface PlanNote {
  code: string;
  params: Record<string, string>;
}

export interface PterodactylConnectionView {
  hasStoredKey: boolean;
}

export interface PlannedPort {
  port: number;
  /** Pterodactyl's primary allocation - the one players connect to. */
  primary: boolean;
  notes: string | null;
}

export interface PlannedEnvironmentVariable {
  key: string;
  value: string;
}

/**
 * `passwordAvailable` is always false and the row says so on purpose:
 * Pterodactyl's Application API never returns database passwords, so the data
 * has to come out of the database server itself.
 */
export interface PlannedDatabase {
  name: string;
  username: string;
  passwordAvailable: boolean;
  /** The machine the panel keeps this database on. */
  hostAddress: string;
  /** The VibeSSH Node that is that machine, when one is. */
  hostServerId: string | null;
  hostServerName: string | null;
}

/** Which machine the server's files are on, and whether VibeSSH knows it. */
export interface PlannedSource {
  nodeName: string;
  fqdn: string;
  volumePath: string;
  matchedServerId: string | null;
  matchedServerName: string | null;
}

export interface PlannedServer {
  sourceId: number;
  sourceUuid: string;
  name: string;
  egg: string;
  suspended: boolean;
  blueprintId: string;
  /** Why that image was chosen. A key, not a sentence. */
  reason: PlanNote;
  fields: Record<string, unknown>;
  ports: PlannedPort[];
  environment: PlannedEnvironmentVariable[];
  memoryMb: number | null;
  cpuCores: number | null;
  databases: PlannedDatabase[];
  source: PlannedSource;
  warnings: PlanNote[];
}

export interface PterodactylMigrationPlan {
  panelUrl: string;
  totalServers: number;
  servers: PlannedServer[];
  notes: PlanNote[];
}

/** Which step of one server's import is happening. */
export type ImportStep = "stopping" | "creating" | "configuring" | "copyingFiles" | "movingDatabases" | "done";

export interface ImportProgress {
  sourceId: number;
  name: string;
  step: ImportStep;
  /** Zero-based, so the interface can say "3 of 12" without counting. */
  index: number;
  total: number;
}

/** What happened to one server. */
export interface ImportOutcome {
  sourceId: number;
  name: string;
  applicationId: string | null;
  filesCopied: number;
  /** New VibeSSH names of the databases that moved. */
  databasesMoved: string[];
  warnings: PlanNote[];
  /** Set when this server did not migrate; the others still may have. */
  failed: PlanNote | null;
}

/**
 * One "this Pterodactyl node is that VibeSSH Node" answer.
 *
 * Needed because the automatic match compares the panel's address to a
 * Node's address, and the same machine is routinely known by a DNS name in
 * one and an IP in the other. Keyed by fqdn, which is what the plan shows.
 */
export interface NodeOverride {
  fqdn: string;
  serverId: string;
}
