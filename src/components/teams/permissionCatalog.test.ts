import { describe, expect, it } from "vitest";
import { groupPermissions, permissionGroup } from "./permissionCatalog";

describe("permissionGroup", () => {
  it("reads the group from the key's own shape", () => {
    expect(permissionGroup("team.view")).toBe("team");
    expect(permissionGroup("team.members.add")).toBe("members");
    expect(permissionGroup("team.roles.assign")).toBe("roles");
    expect(permissionGroup("audit.view")).toBe("audit");
    expect(permissionGroup("servers.manage")).toBe("servers");
  });

  // Invitations are about who gets into the team, so they belong with
  // members rather than in a group of their own.
  it("puts invitations with members", () => {
    expect(permissionGroup("team.invitations.manage")).toBe("members");
  });

  it("has somewhere to put a key it does not recognise", () => {
    expect(permissionGroup("applications.restart")).toBe("other");
  });
});

describe("groupPermissions", () => {
  const catalog = [
    "team.view",
    "team.update",
    "team.delete",
    "team.members.add",
    "team.members.remove",
    "team.roles.manage",
    "team.roles.assign",
    "team.invitations.manage",
    "audit.view",
    "servers.manage",
  ];

  it("orders the groups the way they are declared", () => {
    expect(groupPermissions(catalog).map((entry) => entry.group)).toEqual(["team", "members", "roles", "audit", "servers"]);
  });

  it("keeps the catalog's own order inside a group", () => {
    const team = groupPermissions(catalog).find((entry) => entry.group === "team");
    expect(team?.permissions).toEqual(["team.view", "team.update", "team.delete"]);
  });

  // A permission the form cannot display is a permission nobody can grant,
  // so an unknown key has to survive rather than be filtered out.
  it("shows a permission it has no rule for, in a group at the end", () => {
    const grouped = groupPermissions([...catalog, "applications.restart"]);
    expect(grouped[grouped.length - 1]).toEqual({ group: "other", permissions: ["applications.restart"] });
  });

  it("leaves out a group with nothing in it", () => {
    expect(groupPermissions(["audit.view"]).map((entry) => entry.group)).toEqual(["audit"]);
  });
});
