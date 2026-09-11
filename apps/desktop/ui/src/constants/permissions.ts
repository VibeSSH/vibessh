/** Mirrors backend/src/permissions.rs's catalog - the frontend never invents
 * its own permission strings, only checks against these.
 *
 * `TEAM_VIEW` and `TEAM_UPDATE` are currently checked nowhere, and are kept
 * anyway: the value of this file is that it is the *whole* catalog, so a
 * screen that later needs one of them finds it here instead of typing the
 * string. An incomplete mirror invites exactly the invented string this file
 * exists to prevent. (The audit listed this file as dead code; it is not -
 * see `AUDIT_REPORT.md` §16.) */
export const TEAM_VIEW = "team.view";
export const TEAM_UPDATE = "team.update";
export const TEAM_DELETE = "team.delete";
export const TEAM_MEMBERS_ADD = "team.members.add";
export const TEAM_MEMBERS_REMOVE = "team.members.remove";
export const TEAM_ROLES_MANAGE = "team.roles.manage";
export const AUDIT_VIEW = "audit.view";
export const SERVERS_MANAGE = "servers.manage";
