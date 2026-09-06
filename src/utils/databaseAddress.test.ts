import { describe, expect, it } from "vitest";
import { formatReachableDatabaseAddress, isLoopback, reachableDatabaseAddress } from "./databaseAddress";
import type { DatabaseHost } from "@/types/database";

function host(address: string, port = 3306): DatabaseHost {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    name: "db",
    engine: "mariadb",
    host: address,
    port,
    adminUsername: "root",
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

describe("reachableDatabaseAddress", () => {
  /**
   * The bug this exists for, in one test. A database server on the Node is
   * configured as 127.0.0.1 there, and a container that reads that address
   * gets itself - so it has to be handed the gateway name instead, which
   * every VibeSSH container is given a mapping for.
   */
  it("turns the node's own loopback address into the one a container can use", () => {
    expect(reachableDatabaseAddress(host("127.0.0.1"))).toEqual({ host: "host.docker.internal", port: 3306 });
    expect(reachableDatabaseAddress(host("localhost"))).toEqual({ host: "host.docker.internal", port: 3306 });
    expect(reachableDatabaseAddress(host("::1"))).toEqual({ host: "host.docker.internal", port: 3306 });
  });

  it("leaves an address that already works from both sides alone", () => {
    expect(reachableDatabaseAddress(host("10.0.0.5", 3307))).toEqual({ host: "10.0.0.5", port: 3307 });
    expect(reachableDatabaseAddress(host("db.example.com"))).toEqual({ host: "db.example.com", port: 3306 });
  });

  /**
   * The rule is a prefix match on `127.`, so a hostname that happens to
   * start with it counts as loopback. Pinned rather than fixed: such a name
   * does not exist in practice, and the alternative - parsing an address
   * that may be a name, an IPv4 or an IPv6 - would be a lot of machinery to
   * get the same answer for every real input.
   */
  it("treats anything starting with 127. as loopback, names included", () => {
    expect(isLoopback("127.example.com")).toBe(true);
    // Only at the start, though - these are ordinary addresses.
    expect(isLoopback("10.127.0.1")).toBe(false);
    expect(isLoopback("localhost.example.com")).toBe(false);
  });

  it("formats both halves for display", () => {
    expect(formatReachableDatabaseAddress(host("127.0.0.1", 3307))).toBe("host.docker.internal:3307");
  });
});
