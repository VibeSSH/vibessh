/**
 * How the permission catalog is presented, as distinct from what it is.
 *
 * The backend hands over a flat list of stable keys (`team.roles.manage`,
 * `servers.manage`, ...) and that flatness is right for an API and wrong
 * for a form. Eight-and-growing checkboxes in one undivided grid is a list
 * to read rather than a decision to make; grouped by what they act on, it
 * becomes four short questions.
 *
 * The grouping lives here rather than in the component so the order is one
 * thing to look at, and so a key the backend adds before this file knows
 * about it still appears - in its own group at the end - instead of
 * vanishing from the form.
 */

/** The groups, in the order they are shown. */
export const PERMISSION_GROUPS = ["team", "members", "roles", "audit", "servers", "applications", "node"] as const;

export type PermissionGroup = (typeof PERMISSION_GROUPS)[number] | "other";

/**
 * Which group a key belongs to.
 *
 * Matched on the key's own shape rather than on a hand-written map, so a
 * new `team.members.*` permission lands in Members without anybody
 * remembering to add it.
 */
export function permissionGroup(permission: string): PermissionGroup {
  if (permission.startsWith("team.members.")) return "members";
  if (permission.startsWith("team.roles.")) return "roles";
  if (permission.startsWith("team.invitations.")) return "members";
  if (permission.startsWith("team.")) return "team";
  if (permission.startsWith("audit.")) return "audit";
  if (permission.startsWith("servers.")) return "servers";
  if (permission.startsWith("applications.")) return "applications";
  if (permission.startsWith("node.")) return "node";
  return "other";
}

export interface GroupedPermissions {
  group: PermissionGroup;
  permissions: string[];
}

/**
 * The catalog, grouped and ordered.
 *
 * Keys inside a group keep the backend's own order, which is the order the
 * catalog is written in - roughly least to most powerful, and worth
 * preserving because it reads as an escalation.
 */
export function groupPermissions(permissions: string[]): GroupedPermissions[] {
  const grouped = new Map<PermissionGroup, string[]>();
  for (const permission of permissions) {
    const group = permissionGroup(permission);
    const existing = grouped.get(group);
    if (existing) {
      existing.push(permission);
    } else {
      grouped.set(group, [permission]);
    }
  }

  const ordered: GroupedPermissions[] = [];
  for (const group of PERMISSION_GROUPS) {
    const inGroup = grouped.get(group);
    if (inGroup) ordered.push({ group, permissions: inGroup });
  }
  // Anything the backend added that this file has no rule for - shown, not
  // dropped, because a permission the form cannot display is a permission
  // nobody can grant.
  const rest = grouped.get("other");
  if (rest) ordered.push({ group: "other", permissions: rest });
  return ordered;
}
