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

pub const ALL_PERMISSIONS: &[&str] =
    &[TEAM_VIEW, TEAM_UPDATE, TEAM_DELETE, TEAM_MEMBERS_ADD, TEAM_MEMBERS_REMOVE, TEAM_ROLES_MANAGE];

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
