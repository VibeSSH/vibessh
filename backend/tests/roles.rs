//! Real integration tests for Roles + Permissions, against a real Postgres.
mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{delete, get_with_bearer, patch, post_with_bearer, register_user, test_router};

async fn create_team(owner_token: &str, name: &str) -> serde_json::Value {
    let (status, team) = post_with_bearer(test_router().await, "/teams", owner_token, json!({ "name": name })).await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    team
}

#[tokio::test]
async fn get_permissions_returns_the_catalog() {
    let (_, token) = register_user().await;
    let (status, body) = get_with_bearer(test_router().await, "/permissions", &token).await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().unwrap();
    assert!(list.iter().any(|p| p == "team.roles.manage"));
}

#[tokio::test]
async fn creating_a_team_seeds_an_owner_role_with_every_permission_assigned_to_the_creator() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Owned Team").await;
    let team_id = team["id"].as_str().unwrap();
    let owner_id = team["ownerId"].as_str().unwrap();

    let (status, roles) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/roles"), &owner_token).await;
    assert_eq!(status, StatusCode::OK);
    let roles = roles.as_array().unwrap();
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0]["name"], "Owner");
    assert_eq!(roles[0]["isSystem"], true);
    assert!(roles[0]["permissions"].as_array().unwrap().contains(&json!("team.delete")));

    let (status, member_roles) =
        get_with_bearer(test_router().await, &format!("/teams/{team_id}/members/{owner_id}/roles"), &owner_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(member_roles.as_array().unwrap().len(), 1);
    assert_eq!(member_roles.as_array().unwrap()[0]["name"], "Owner");
}

#[tokio::test]
async fn owner_can_create_a_custom_role_with_a_subset_of_permissions() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (status, role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "Viewer", "description": "Read only", "permissions": ["team.view"] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{role}");
    assert_eq!(role["name"], "Viewer");
    assert_eq!(role["isSystem"], false);
    assert_eq!(role["permissions"], json!(["team.view"]));
}

#[tokio::test]
async fn a_duplicate_permission_in_the_request_is_deduplicated_not_a_server_error() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (status, role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "Deduped", "permissions": ["team.view", "team.view", "team.update"] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{role}");
    let perms = role["permissions"].as_array().unwrap();
    assert_eq!(perms.len(), 2);
}

#[tokio::test]
async fn creating_a_role_with_an_unknown_permission_is_rejected() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (status, body) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "Bogus", "permissions": ["not.a.real.permission"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn the_name_owner_is_reserved_and_cannot_be_used_for_a_custom_role() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (status, body) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "owner", "permissions": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn duplicate_role_names_within_a_team_are_a_conflict() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let make = || json!({ "name": "Deployer", "permissions": [] });
    let (status, _) = post_with_bearer(test_router().await, &format!("/teams/{team_id}/roles"), &owner_token, make()).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = post_with_bearer(test_router().await, &format!("/teams/{team_id}/roles"), &owner_token, make()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn a_non_owner_member_cannot_create_roles() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, member_token) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;

    let (status, _) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &member_token,
        json!({ "name": "Sneaky", "permissions": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn owner_can_update_a_custom_roles_permissions() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (_, role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "Editable", "permissions": ["team.view"] }),
    )
    .await;
    let role_id = role["id"].as_str().unwrap();

    let (status, updated) = patch(
        test_router().await,
        &format!("/teams/{team_id}/roles/{role_id}"),
        &owner_token,
        json!({ "name": "Editable", "permissions": ["team.view", "team.update"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    let perms = updated["permissions"].as_array().unwrap();
    assert_eq!(perms.len(), 2);
}

#[tokio::test]
async fn the_owner_role_cannot_be_updated_or_deleted() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (_, roles) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/roles"), &owner_token).await;
    let owner_role_id = roles.as_array().unwrap()[0]["id"].as_str().unwrap();

    let (status, _) = patch(
        test_router().await,
        &format!("/teams/{team_id}/roles/{owner_role_id}"),
        &owner_token,
        json!({ "name": "Owner", "permissions": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}/roles/{owner_role_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn deleting_a_custom_role_removes_it_from_the_list() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (_, role) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/roles"), &owner_token, json!({ "name": "Temp", "permissions": [] }))
            .await;
    let role_id = role["id"].as_str().unwrap();

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}/roles/{role_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, roles) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/roles"), &owner_token).await;
    // Only the seeded Owner role remains.
    assert_eq!(roles.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn owner_can_assign_a_custom_role_to_a_member_and_then_unassign_it() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, _) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;
    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    let member_id = members.as_array().unwrap().iter().find(|m| m["email"] == member_email).unwrap()["userId"].as_str().unwrap();

    let (_, role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "Viewer", "permissions": ["team.view"] }),
    )
    .await;
    let role_id = role["id"].as_str().unwrap();

    let (status, _) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members/{member_id}/roles"),
        &owner_token,
        json!({ "roleId": role_id }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, member_roles) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members/{member_id}/roles"), &owner_token).await;
    assert_eq!(member_roles.as_array().unwrap().len(), 1);

    let (status, _) =
        delete(test_router().await, &format!("/teams/{team_id}/members/{member_id}/roles/{role_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, member_roles_after) =
        get_with_bearer(test_router().await, &format!("/teams/{team_id}/members/{member_id}/roles"), &owner_token).await;
    assert_eq!(member_roles_after.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn assigning_a_role_to_someone_who_isnt_a_member_is_not_found() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (_, role) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/roles"), &owner_token, json!({ "name": "Viewer", "permissions": [] }))
            .await;
    let role_id = role["id"].as_str().unwrap();

    let (_, outsider_token) = register_user().await;
    let outsider_profile = get_with_bearer(test_router().await, "/auth/me", &outsider_token).await.1;
    let outsider_id = outsider_profile["id"].as_str().unwrap();

    let (status, body) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members/{outsider_id}/roles"),
        &owner_token,
        json!({ "roleId": role_id }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn a_role_from_a_different_team_cannot_be_assigned() {
    let (_, owner_a_token) = register_user().await;
    let team_a = create_team(&owner_a_token, "Team A").await;
    let team_a_id = team_a["id"].as_str().unwrap();

    let (_, owner_b_token) = register_user().await;
    let team_b = create_team(&owner_b_token, "Team B").await;
    let team_b_id = team_b["id"].as_str().unwrap();
    let owner_b_id = team_b["ownerId"].as_str().unwrap();

    // A role that belongs to Team A...
    let (_, role_a) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_a_id}/roles"),
        &owner_a_token,
        json!({ "name": "TeamAOnly", "permissions": [] }),
    )
    .await;
    let role_a_id = role_a["id"].as_str().unwrap();

    // ...cannot be assigned to a member of Team B, even by Team B's own owner.
    let (status, body) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_b_id}/members/{owner_b_id}/roles"),
        &owner_b_token,
        json!({ "roleId": role_a_id }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn the_owners_own_role_assignment_cannot_be_unassigned() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();
    let owner_id = team["ownerId"].as_str().unwrap();

    let (_, roles) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/roles"), &owner_token).await;
    let owner_role_id = roles.as_array().unwrap()[0]["id"].as_str().unwrap();

    let (status, body) = delete(
        test_router().await,
        &format!("/teams/{team_id}/members/{owner_id}/roles/{owner_role_id}"),
        &owner_token,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}
