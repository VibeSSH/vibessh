import { describe, expect, it } from "vitest";
import { connectionAddressFor } from "./DatabasesTab";
import type { ApplicationDatabase, DatabaseHost } from "@/types/database";

const t = (key: string) => key;

function host(overrides: Partial<DatabaseHost>): DatabaseHost {
  return { id: "host-1", name: "db", engine: "mariadb", host: "127.0.0.1", port: 3306, ...overrides } as DatabaseHost;
}

const database = { id: "db-1", databaseHostId: "host-1", databaseName: "app", username: "app_user" } as ApplicationDatabase;

/**
 * The credentials card used to offer one field, labelled "Host", holding
 * `host.docker.internal:3307`. Somebody copied it into a `MYSQL_HOST`
 * variable that had a `MYSQL_PORT` beside it, and the plugin spent the
 * evening failing to open a socket. Both shapes are offered now, and these
 * pin that they stay separable.
 */
describe("connectionAddressFor", () => {
  it("offers the host without the port glued to it", () => {
    const address = connectionAddressFor(database, [host({ port: 3307 })], t);

    expect(address.host).toBe("host.docker.internal");
    expect(address.port).toBe("3307");
    expect(address.combined).toBe("host.docker.internal:3307");
  });

  /** A database server somewhere other than the Node keeps its own address -
   *  `host.docker.internal` would be the container itself's host, not that
   *  machine. */
  it("keeps a non-loopback address as it is", () => {
    const address = connectionAddressFor(database, [host({ host: "10.77.0.2" })], t);

    expect(address.host).toBe("10.77.0.2");
    expect(address.combined).toBe("10.77.0.2:3306");
  });

  /** The host list arrives from its own query and can be a render behind. */
  it("says so in every field when the host is not among those loaded", () => {
    const address = connectionAddressFor(database, [], t);

    expect(address).toEqual({ host: "databasesTab.unknownHost", port: "databasesTab.unknownHost", combined: "databasesTab.unknownHost" });
  });
});
