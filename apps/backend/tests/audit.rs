//! Real integration tests for the Audit Log, against a real Postgres.
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

use common::{get_with_bearer, post_with_bearer, register_user, test_router};

async fn create_team(owner_token: &str, name: &str) -> serde_json::Value {
    let (status, team) = post_with_bearer(test_router().await, "/teams", owner_token, json!({ "name": name })).await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    team
}

#[tokio::test]
#[ignore]
async fn creating_a_team_is_recorded_in_its_own_audit_log() {
    let (owner_email, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Audited Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (status, events) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/audit"), &owner_token).await;
    assert_eq!(status, StatusCode::OK, "{events}");
    let events = events.as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["action"], "team.created");
    assert_eq!(events[0]["targetType"], "team");
    assert_eq!(events[0]["result"], "success");
    assert_eq!(events[0]["actorEmail"], owner_email);
}

#[tokio::test]
#[ignore]
async fn member_role_and_role_assignment_actions_are_all_recorded_in_order() {
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

    post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members/{member_id}/roles"),
        &owner_token,
        json!({ "roleId": role_id }),
    )
    .await;

    let (status, events) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/audit"), &owner_token).await;
    assert_eq!(status, StatusCode::OK);
    let actions: Vec<&str> = events.as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();

    // Newest first - role.assigned was the last thing that happened.
    assert_eq!(actions, vec!["role.assigned", "role.created", "member.added", "team.created"]);
}

#[tokio::test]
#[ignore]
async fn a_member_without_audit_view_cannot_read_the_audit_log() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, member_token) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;

    let (status, _) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/audit"), &member_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
#[ignore]
async fn granting_audit_view_lets_a_non_owner_member_read_the_log() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, member_token) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;
    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    let member_id = members.as_array().unwrap().iter().find(|m| m["email"] == member_email).unwrap()["userId"].as_str().unwrap();

    let (_, role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "Auditor", "permissions": ["audit.view"] }),
    )
    .await;
    let role_id = role["id"].as_str().unwrap();
    post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members/{member_id}/roles"),
        &owner_token,
        json!({ "roleId": role_id }),
    )
    .await;

    let (status, events) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/audit"), &member_token).await;
    assert_eq!(status, StatusCode::OK, "{events}");
    assert!(!events.as_array().unwrap().is_empty());
}

#[tokio::test]
#[ignore]
async fn a_non_member_cannot_read_the_audit_log() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (_, outsider_token) = register_user().await;
    let (status, _) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/audit"), &outsider_token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore]
async fn the_limit_query_parameter_caps_how_many_events_come_back() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    // team.created is already one event; create two roles for two more.
    for name in ["Role A", "Role B"] {
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/roles"), &owner_token, json!({ "name": name, "permissions": [] }))
            .await;
    }

    let (status, events) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/audit?limit=2"), &owner_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(events.as_array().unwrap().len(), 2);
}

#[tokio::test]
#[ignore]
async fn removing_a_member_and_unassigning_a_role_are_both_recorded() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, _) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;
    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    let member_id = members.as_array().unwrap().iter().find(|m| m["email"] == member_email).unwrap()["userId"].as_str().unwrap().to_string();

    let (_, role) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/roles"), &owner_token, json!({ "name": "R", "permissions": [] }))
            .await;
    let role_id = role["id"].as_str().unwrap().to_string();
    post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members/{member_id}/roles"),
        &owner_token,
        json!({ "roleId": role_id }),
    )
    .await;

    common::delete(test_router().await, &format!("/teams/{team_id}/members/{member_id}/roles/{role_id}"), &owner_token).await;
    common::delete(test_router().await, &format!("/teams/{team_id}/members/{member_id}"), &owner_token).await;

    let (_, events) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/audit"), &owner_token).await;
    let actions: Vec<&str> = events.as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert!(actions.contains(&"member.removed"));
    assert!(actions.contains(&"role.unassigned"));
}
