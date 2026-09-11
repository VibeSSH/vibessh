import { describe, expect, it } from "vitest";
import { connectionNote } from "./EnvironmentTab";
import type { ApplicationDetail, Blueprint, EnvironmentVariable } from "@/types/application";

function application(environment: EnvironmentVariable[]): ApplicationDetail {
  return { environment } as unknown as ApplicationDetail;
}

function blueprint(connectsTo: Blueprint["connectsTo"]): Blueprint {
  return { id: "phpmyadmin", connectsTo } as unknown as Blueprint;
}

const PHPMYADMIN = { blueprintIds: ["mariadb"], hostEnv: "PMA_HOST", portEnv: "PMA_PORT", defaultPort: 3306 };

/**
 * The note beside the two variables nobody typed.
 *
 * Somebody published their MariaDB on 3307, came to this tab expecting
 * `PMA_PORT` to have followed, and reported that it had not. It had not, and
 * should not: that port is the one inside the target's own container. The
 * note exists to say so, and these pin when it appears - a blueprint that
 * points at nothing must not grow an explanation of a connection it does not
 * have.
 */
describe("connectionNote", () => {
  it("names both variables for an application whose blueprint points at another", () => {
    const note = connectionNote(
      application([{ key: "PMA_HOST", value: "mariadb-x", isSecret: false }]),
      blueprint(PHPMYADMIN),
    );

    expect(note).toEqual({ host: "PMA_HOST", port: "PMA_PORT" });
  });

  it("says nothing for a blueprint that points at nothing", () => {
    expect(connectionNote(application([{ key: "MYSQL_ROOT_PASSWORD", value: "x", isSecret: true }]), blueprint(undefined))).toBeNull();
  });

  /** The blueprint arrives one render after the application does. */
  it("says nothing before the blueprint has loaded", () => {
    expect(connectionNote(application([{ key: "PMA_HOST", value: "mariadb-x", isSecret: false }]), null)).toBeNull();
  });

  /** Both rows can be deleted on this very tab. Explaining variables that are
   *  no longer there would be an explanation of nothing. */
  it("says nothing once neither variable is left", () => {
    expect(connectionNote(application([{ key: "PMA_ARBITRARY", value: "1", isSecret: false }]), blueprint(PHPMYADMIN))).toBeNull();
  });

  /** Only the port is deleted, which is exactly when somebody is mid-edit and
   *  most needs to read what the host beside it means. */
  it("still explains when only one of the two survives", () => {
    const note = connectionNote(application([{ key: "PMA_PORT", value: "3306", isSecret: false }]), blueprint(PHPMYADMIN));

    expect(note).toEqual({ host: "PMA_HOST", port: "PMA_PORT" });
  });
});
