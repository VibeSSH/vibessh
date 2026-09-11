//! The permission catalog - a reviewable, versioned list in code, not a
//! database table that could silently drift from what the backend actually
//! checks (see the production roadmap's Domain & Database Architecture
//! section for why this is relational-with-string-keys rather than a
//! bitmask: this list is already going to grow past what comfortably fits
//! in a 32/64-bit flag set as more resource types - servers, applications,
//! databases - get their own backend-tracked permissions in later stages).
//! Scoped for now to exactly what this backend has real resources for:
//! teams themselves. `role_permissions.permission_key` is a plain TEXT
//! column; every write path validates against this list rather than trusting
//! whatever string a client sends.

pub const TEAM_VIEW: &str = "team.view";
pub const TEAM_UPDATE: &str = "team.update";
pub const TEAM_DELETE: &str = "team.delete";
pub const TEAM_MEMBERS_ADD: &str = "team.members.add";
pub const TEAM_MEMBERS_REMOVE: &str = "team.members.remove";
pub const TEAM_ROLES_MANAGE: &str = "team.roles.manage";
/// Hand out roles that already exist, without being able to define new ones.
///
/// Split out of `TEAM_ROLES_MANAGE` because they are different powers:
/// deciding what a role may do is a policy decision, giving somebody an
/// existing role is day-to-day administration, and a team lead usually
/// wants the second without the first. `TEAM_ROLES_MANAGE` still implies
/// this - see `authorize_any` at the call sites - so no existing role loses
/// anything by this existing.
pub const TEAM_ROLES_ASSIGN: &str = "team.roles.assign";
pub const AUDIT_VIEW: &str = "audit.view";
pub const SERVERS_MANAGE: &str = "servers.manage";

// --- Operations the desktop app performs directly on a Node -------------
//
// These are in the catalog so a role can carry them and this backend will
// accept and store them, but **this backend does not and cannot enforce
// them**. The operations they name - creating an application, opening a
// port, changing a firewall rule, opening a terminal - never reach here:
// the desktop app runs them over its own SSH connection, with the
// operator's own credentials, from the operator's own machine. See
// `migrations/0006_team_servers.sql` for why that is the design: the
// backend holds metadata for team visibility and never holds credentials.
//
// So what are they for? They are guard rails inside the app: they hide and
// disable actions for a member who has not been given them, which stops a
// colleague doing something by accident. They are not a security boundary,
// because anyone able to reach the Node over SSH can do the same thing
// without VibeSSH at all. That distinction is stated in the interface next
// to the checkboxes and in the guide - it must never be quietly dropped
// from either.
pub const APPLICATIONS_VIEW: &str = "applications.view";
pub const APPLICATIONS_CREATE: &str = "applications.create";
pub const APPLICATIONS_LIFECYCLE: &str = "applications.lifecycle";
pub const APPLICATIONS_DELETE: &str = "applications.delete";
pub const APPLICATIONS_CONFIG: &str = "applications.config";
pub const APPLICATIONS_PORTS: &str = "applications.ports";
pub const APPLICATIONS_FILES_READ: &str = "applications.files.read";
pub const APPLICATIONS_FILES_WRITE: &str = "applications.files.write";
pub const APPLICATIONS_BACKUPS: &str = "applications.backups";
pub const APPLICATIONS_DATABASES: &str = "applications.databases";
pub const NODE_TERMINAL: &str = "node.terminal";
pub const NODE_FIREWALL: &str = "node.firewall";
pub const NODE_SERVICES: &str = "node.services";
pub const NODE_SOFTWARE: &str = "node.software";
pub const NODE_NETWORK: &str = "node.network";

pub const ALL_PERMISSIONS: &[&str] = &[
    TEAM_VIEW,
    TEAM_UPDATE,
    TEAM_DELETE,
    TEAM_MEMBERS_ADD,
    TEAM_MEMBERS_REMOVE,
    TEAM_ROLES_MANAGE,
    TEAM_ROLES_ASSIGN,
    AUDIT_VIEW,
    SERVERS_MANAGE,
    APPLICATIONS_VIEW,
    APPLICATIONS_CREATE,
    APPLICATIONS_LIFECYCLE,
    APPLICATIONS_DELETE,
    APPLICATIONS_CONFIG,
    APPLICATIONS_PORTS,
    APPLICATIONS_FILES_READ,
    APPLICATIONS_FILES_WRITE,
    APPLICATIONS_BACKUPS,
    APPLICATIONS_DATABASES,
    NODE_TERMINAL,
    NODE_FIREWALL,
    NODE_SERVICES,
    NODE_SOFTWARE,
    NODE_NETWORK,
];

/// The permissions this backend stores but does not check, because the
/// operations they name never reach it - see their declarations above.
///
/// Public and named rather than left implicit: it is the list somebody has
/// to read before believing that ticking a box here stops anything on a
/// Node.
pub const ENFORCED_BY_THE_DESKTOP_APP: &[&str] = &[
    APPLICATIONS_VIEW,
    APPLICATIONS_CREATE,
    APPLICATIONS_LIFECYCLE,
    APPLICATIONS_DELETE,
    APPLICATIONS_CONFIG,
    APPLICATIONS_PORTS,
    APPLICATIONS_FILES_READ,
    APPLICATIONS_FILES_WRITE,
    APPLICATIONS_BACKUPS,
    APPLICATIONS_DATABASES,
    NODE_TERMINAL,
    NODE_FIREWALL,
    NODE_SERVICES,
    NODE_SOFTWARE,
    NODE_NETWORK,
];

pub fn is_known_permission(key: &str) -> bool {
    ALL_PERMISSIONS.contains(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_entry_is_recognized_and_nothing_else_is() {
        for permission in ALL_PERMISSIONS {
            assert!(is_known_permission(permission));
        }
        assert!(!is_known_permission("not.a.real.permission"));
    }
}

#[cfg(test)]
mod catalog_growth_tests {
    use super::*;

    /// Catalog entries no handler checks, and why.
    ///
    /// `team.view` is granted structurally rather than by role: every read
    /// endpoint calls `teams::team_for_member`, which returns "not found"
    /// to anybody who is not a member of the team. So membership *is* the
    /// view check, and the permission is a label for something already
    /// true - unchecking it removes nothing.
    ///
    /// `team.update` guards an endpoint that does not exist: nothing in
    /// this backend renames or otherwise edits a team, so there is no call
    /// site to put the check in. It is a placeholder for a feature, and the
    /// checkbox in the UI is currently inert.
    ///
    /// Both stay in the catalog rather than being deleted because teams in
    /// the wild already have roles holding them: dropping a key would make
    /// `is_known_permission` reject their next role update. They are listed
    /// here so the gap is written down instead of being discovered by
    /// somebody who ticks the box and expects an effect.
    const NOT_ENFORCED_BY_A_HANDLER: &[&str] = &[TEAM_VIEW, TEAM_UPDATE];

    /// A permission that no handler ever checks is a checkbox that lies.
    ///
    /// This does not prove enforcement - only that a key is used somewhere
    /// outside this file. It is enough to catch the real mistake: adding a
    /// key to the catalog, shipping the checkbox, and never wiring it up.
    #[test]
    fn every_permission_is_referenced_by_a_handler() {
        let sources = [
            include_str!("teams.rs"),
            include_str!("roles.rs"),
            include_str!("audit.rs"),
            include_str!("team_servers.rs"),
        ];
        for permission in ALL_PERMISSIONS {
            if NOT_ENFORCED_BY_A_HANDLER.contains(permission) || ENFORCED_BY_THE_DESKTOP_APP.contains(permission) {
                continue;
            }
            // The constants are referenced by name, not by their string
            // value, so search for the name.
            let constant = permission.to_uppercase().replace('.', "_");
            assert!(
                sources.iter().any(|source| source.contains(&constant)),
                "{permission} is in the catalog but no handler references permissions::{constant} - \
                 either enforce it or take it out of the catalog"
            );
        }
    }

    /// The two permissions carved out of wider ones must stay carved out:
    /// each of their call sites has to accept the wider permission too, or
    /// the split silently revokes access from every team already configured.
    #[test]
    fn split_permissions_still_accept_the_permission_they_came_from() {
        assert!(include_str!("roles.rs").contains("TEAM_ROLES_MANAGE, permissions::TEAM_ROLES_ASSIGN"));
    }
}

#[cfg(test)]
mod desktop_permission_tests {
    use super::*;

    /// Every desktop-enforced permission is in the catalog, so a role
    /// carrying one is accepted and stored rather than rejected as unknown.
    #[test]
    fn the_desktop_list_is_a_subset_of_the_catalog() {
        for permission in ENFORCED_BY_THE_DESKTOP_APP {
            assert!(is_known_permission(permission), "{permission} is enforced by the desktop app but is not in the catalog");
        }
    }

    /// The two lists must not overlap. A permission this backend enforces
    /// and also delegates would be a claim about where the boundary is that
    /// nobody could check.
    #[test]
    fn nothing_is_both_enforced_here_and_delegated() {
        let backend_enforced: Vec<&&str> =
            ALL_PERMISSIONS.iter().filter(|permission| !ENFORCED_BY_THE_DESKTOP_APP.contains(permission)).collect();
        for permission in &backend_enforced {
            assert!(!ENFORCED_BY_THE_DESKTOP_APP.contains(permission), "{permission} is in both lists");
        }
        // And the split accounts for everything: catalog = backend + desktop.
        assert_eq!(backend_enforced.len() + ENFORCED_BY_THE_DESKTOP_APP.len(), ALL_PERMISSIONS.len());
    }
}
