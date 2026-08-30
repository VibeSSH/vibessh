//! Proves the RBAC wiring is real, not just a refactor of the same
//! owner-only checks under a new name: a non-owner member granted a custom
//! role with the right permission can perform an action that used to be
//! owner-exclusive, and a member without that permission still can't -
//! against a real Postgres, through the real HTTP router.
mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{delete, get_with_bearer, post_with_bearer, register_user, test_router};

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
    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), owner_token).await;
    members.as_array().unwrap().iter().find(|m| m["email"] == email).unwrap()["userId"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_member_granted_team_members_add_can_add_members_without_being_the_owner() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Delegated Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (delegate_email, delegate_token) = register_user().await;
    add_member(&owner_token, team_id, &delegate_email).await;
    let delegate_id = member_id(&owner_token, team_id, &delegate_email).await;

    // Before being granted the permission, the delegate can't add members -
    // same as any other non-owner member.
    let (third_email, _) = register_user().await;
    let (status, _) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &delegate_token, json!({ "email": third_email })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Owner creates a custom role with exactly one permission and assigns it.
    let (_, role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "Recruiter", "permissions": ["team.members.add"] }),
    )
    .await;
    let role_id = role["id"].as_str().unwrap();
    let (status, _) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members/{delegate_id}/roles"),
        &owner_token,
        json!({ "roleId": role_id }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Now the same delegate, still not the owner, can add a member for real.
    let (status, body) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &delegate_token, json!({ "email": third_email })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    assert_eq!(members.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn a_role_that_grants_team_members_add_does_not_also_grant_team_members_remove() {
    // Permissions are specific, not a package deal - a role scoped to one
    // action shouldn't silently unlock a different one.
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (delegate_email, delegate_token) = register_user().await;
    add_member(&owner_token, team_id, &delegate_email).await;
    let delegate_id = member_id(&owner_token, team_id, &delegate_email).await;

    let (_, role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "AdderOnly", "permissions": ["team.members.add"] }),
    )
    .await;
    let role_id = role["id"].as_str().unwrap();
    post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members/{delegate_id}/roles"),
        &owner_token,
        json!({ "roleId": role_id }),
    )
    .await;

    let (other_email, _) = register_user().await;
    add_member(&owner_token, team_id, &other_email).await;
    let other_id = member_id(&owner_token, team_id, &other_email).await;

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}/members/{other_id}"), &delegate_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn my_permissions_reflects_the_owners_full_permission_set() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (status, perms) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/me/permissions"), &owner_token).await;
    assert_eq!(status, StatusCode::OK);
    let perms = perms.as_array().unwrap();
    assert!(perms.iter().any(|p| p == "team.delete"));
    assert!(perms.iter().any(|p| p == "team.roles.manage"));
}

#[tokio::test]
async fn my_permissions_is_empty_for_a_member_with_no_roles_assigned() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, member_token) = register_user().await;
    add_member(&owner_token, team_id, &member_email).await;

    let (status, perms) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/me/permissions"), &member_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(perms.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn my_permissions_for_a_non_member_is_not_found() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (_, outsider_token) = register_user().await;
    let (status, _) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/me/permissions"), &outsider_token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
