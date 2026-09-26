/** Mirrors apps/backend/src/permissions.rs's catalog - the frontend never invents
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
/** Sharing an application with a team, and managing who on that team may see
 * it. Genuinely backend-enforced for the team-application endpoints - see
 * `apps/backend/src/team_applications.rs` - unlike the other `applications.*`
 * keys, which are desktop-side guard rails. */
export const APPLICATIONS_CREATE = "applications.create";
export const APPLICATIONS_LIFECYCLE = "applications.lifecycle";
export const APPLICATIONS_CONSOLE = "applications.console";
export const APPLICATIONS_FILES_READ = "applications.files.read";
export const APPLICATIONS_FILES_WRITE = "applications.files.write";

/** The permissions that can be granted on one shared application rather than
 * team-wide - the backend's `APPLICATION_SCOPED`, in the order the Users tab
 * shows them. Everything else reaches every application or is root on the
 * Node, and is given through a role. */
export const APPLICATION_SCOPED = [APPLICATIONS_LIFECYCLE, APPLICATIONS_CONSOLE, APPLICATIONS_FILES_READ, APPLICATIONS_FILES_WRITE] as const;
