//! Regression tests from a security review of the Roles/Permissions/
//! Invitations stages: a member holding `team.roles.manage` (a permission
//! meant to let a trusted delegate organize roles) must never be able to
//! grant themselves - or anyone else - a permission they don't already
//! hold. Every test here reproduces a real escalation path the review
//! found, against a real Postgres.
//!
//! `#[ignore]` - every test here needs a reachable Postgres named by
//! `DATABASE_URL` (see `backend/.env.example`), which no plain developer
//! checkout has. Without the marker these panic in `common::database_url`
//! and, because cargo stops at the first failing test binary, they took the
//! rest of `cargo test --workspace` down with them. Same convention the
//! real-host tests in `apps/desktop/src-tauri/tests/` already use. Run them explicitly:
//! `DATABASE_URL=... cargo test -p vibessh-backend -- --ignored`.
mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{post_with_bearer, register_user, test_router, unique_email};

async fn create_team(owner_token: &str, name: &str) -> serde_json::Value {
    let (status, team) = post_with_bearer(test_router().await, "/teams", owner_token, json!({ "name": name })).await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    team
}

async fn add_member(owner_token: &str, team_id: &str, email: &str) {
    let (status, body) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), owner_token, json!({ "email": email })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

async fn member_id(owner_token: &str, team_id: &str, email: &str) -> String {
    let (_, members) = common::get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), owner_token).await;
    members.as_array().unwrap().iter().find(|m| m["email"] == email).unwrap()["userId"].as_str().unwrap().to_string()
}

/// The same escalation, through the door that still exists.
///
/// This used to go through `create_invitation`, which was removed when
/// `provision_member` replaced it. The endpoint changed; the risk did not.
/// Provisioning takes a role and assigns it in the same step, so somebody
/// holding only `team.members.add` could otherwise mint an account carrying
/// permissions they do not have themselves and log in as it.
#[tokio::test]
#[ignore]
async fn provisioning_someone_with_a_role_beyond_the_provisioners_own_permissions_is_rejected() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    // `team.members.add` is what provisioning is gated on - grant only that.
    let (delegate_email, delegate_token) = register_user().await;
    add_member(&owner_token, team_id, &delegate_email).await;
    let delegate_id = member_id(&owner_token, team_id, &delegate_email).await;
    let (_, add_role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "Adder", "permissions": ["team.members.add"] }),
    )
    .await;
    post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members/{delegate_id}/roles"),
        &owner_token,
        json!({ "roleId": add_role["id"] }),
    )
    .await;

    let (_, powerful_role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "Powerful", "permissions": ["team.delete"] }),
    )
    .await;

    let (status, body) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members/provision"),
        &delegate_token,
        json!({ "email": unique_email(), "roleId": powerful_role["id"] }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
#[ignore]
async fn the_owner_themselves_is_unaffected_and_can_still_create_and_assign_any_role() {
    // The fix must not accidentally block the one person who's supposed to
    // be able to do all of this - the owner's own Owner role already grants
    // every permission, so every check should pass for them same as before.
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (status, role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "FullPower", "permissions": ["team.delete", "team.members.remove", "team.roles.manage"] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}", body = role);

    let (member_email, _) = register_user().await;
    add_member(&owner_token, team_id, &member_email).await;
    let member_id = member_id(&owner_token, team_id, &member_email).await;

    let (status, _) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members/{member_id}/roles"),
        &owner_token,
        json!({ "roleId": role["id"] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}
