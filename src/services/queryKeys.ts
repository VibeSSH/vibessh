/**
 * Every query key in one place, so invalidation is something you can read.
 *
 * The keys are nested rather than flat - `["application", id, "ports"]`, not
 * `["application-ports", id]` - because that is what makes "this application
 * changed, drop everything about it" a single call. A save that recreates a
 * container invalidates `["application", id]` and the ports, links, files
 * and status underneath it all follow, without anyone having to remember the
 * list.
 */
export const queryKeys = {
  /** Every application, for the dashboard and for pickers. */
  applications: () => ["applications"] as const,
  application: (id: string) => ["application", id] as const,
  applicationPorts: (id: string) => ["application", id, "ports"] as const,
  applicationLinks: (id: string) => ["application", id, "links"] as const,
  applicationDatabases: (id: string) => ["application", id, "databases"] as const,
  applicationBackups: (id: string) => ["application", id, "backups"] as const,
  /** One directory of one application's files - the path is part of the key
   * because each directory is its own answer from the Node. */
  applicationFiles: (id: string, path: string) => ["application", id, "files", path] as const,

  servers: () => ["servers"] as const,
  server: (id: string) => ["server", id] as const,
} as const;
