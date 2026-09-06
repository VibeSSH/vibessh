import type { DatabaseHost } from "@/types/database";

/**
 * Whether an address means "this machine" - and so means the container
 * itself once an application reads it from inside one.
 */
export function isLoopback(address: string): boolean {
  const value = address.trim().toLowerCase();
  return value === "localhost" || value === "::1" || value.startsWith("127.");
}

/**
 * The address an application must use to reach a database host, split into
 * the two halves a connection actually needs.
 *
 * **Not the same as the database host's own address.** Applications run in
 * containers, where `127.0.0.1` is the container and not the Node - so a
 * database server on the Node is reached at `host.docker.internal`, which
 * every container VibeSSH creates is given a mapping for
 * (`--add-host host.docker.internal:host-gateway`, see `runtime::docker`).
 * A database server anywhere else keeps its own address, which works from
 * both sides.
 *
 * This lives here rather than in the Databases tab because the wizard needs
 * the same answer: a phpMyAdmin pointed at a database host has to be given
 * this address and no other. Getting it wrong is the single most reported
 * broken setup in the app - `PMA_HOST=db`, copied from a compose file,
 * resolving to nothing.
 */
export function reachableDatabaseAddress(host: DatabaseHost): { host: string; port: number } {
  return { host: isLoopback(host.host) ? "host.docker.internal" : host.host, port: host.port };
}

/** The same answer as one `host:port` string, for display. */
export function formatReachableDatabaseAddress(host: DatabaseHost): string {
  const address = reachableDatabaseAddress(host);
  return `${address.host}:${address.port}`;
}
