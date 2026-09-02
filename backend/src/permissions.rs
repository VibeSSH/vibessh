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
/// Create, list and revoke invitations.
///
/// Also additive: this was `TEAM_MEMBERS_ADD`, which conflates "may bring
/// somebody into the team" with "may see and cancel everybody's pending
/// invitations". Roles holding `TEAM_MEMBERS_ADD` keep both.
pub const TEAM_INVITATIONS_MANAGE: &str = "team.invitations.manage";
pub const AUDIT_VIEW: &str = "audit.view";
pub const SERVERS_MANAGE: &str = "servers.manage";

pub const ALL_PERMISSIONS: &[&str] = &[
    TEAM_VIEW,
    TEAM_UPDATE,
    TEAM_DELETE,
    TEAM_MEMBERS_ADD,
    TEAM_MEMBERS_REMOVE,
    TEAM_ROLES_MANAGE,
    TEAM_ROLES_ASSIGN,
    TEAM_INVITATIONS_MANAGE,
    AUDIT_VIEW,
    SERVERS_MANAGE,
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
            include_str!("invitations.rs"),
            include_str!("audit.rs"),
            include_str!("team_servers.rs"),
        ];
        for permission in ALL_PERMISSIONS {
            if NOT_ENFORCED_BY_A_HANDLER.contains(permission) {
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
        assert!(include_str!("invitations.rs").contains("TEAM_MEMBERS_ADD, permissions::TEAM_INVITATIONS_MANAGE"));
    }
}
