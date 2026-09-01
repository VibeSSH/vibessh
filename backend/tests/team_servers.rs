//! Real integration tests for team-scoped server metadata, against a real
//! Postgres.
//!
//! `#[ignore]` - every test here needs a reachable Postgres named by
//! `DATABASE_URL` (see `backend/.env.example`), which no plain developer
//! checkout has. Without the marker these panic in `common::database_url`
//! and, because cargo stops at the first failing test binary, they took the
//! rest of `cargo test --workspace` down with them. Same convention the
//! real-host tests in `src-tauri/tests/` already use. Run them explicitly:
//! `DATABASE_URL=... cargo test -p vibessh-backend -- --ignored`.
mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{delete, get_with_bearer, post_with_bearer, register_user, test_router};

async fn create_team(owner_token: &str, name: &str) -> serde_json::Value {
    let (status, team) = post_with_bearer(test_router().await, "/teams", owner_token, json!({ "name": name })).await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    team
}

#[tokio::test]
#[ignore]
async fn owner_can_add_a_server_and_it_appears_in_the_teams_list() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (status, server) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/servers"),
        &owner_token,
        json!({ "name": "Prod DB", "host": "10.0.0.5", "sshPort": 2222, "username": "root" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{server}");
    assert_eq!(server["name"], "Prod DB");
    assert_eq!(server["host"], "10.0.0.5");
    assert_eq!(server["sshPort"], 2222);
    // No secret fields exist on the response at all.
    assert!(server.get("password").is_none());
    assert!(server.get("privateKeyPath").is_none());

    let (status, servers) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/servers"), &owner_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(servers.as_array().unwrap().len(), 1);
}

#[tokio::test]
#[ignore]
async fn defaults_to_port_22_when_not_specified() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (_, server) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/servers"), &owner_token, json!({ "name": "S", "host": "h" })).await;
    assert_eq!(server["sshPort"], 22);
}

#[tokio::test]
#[ignore]
async fn a_non_owner_member_cannot_add_a_server() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, member_token) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;

    let (status, _) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/servers"),
        &member_token,
        json!({ "name": "S", "host": "h" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
#[ignore]
async fn a_non_member_cannot_list_servers() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (_, outsider_token) = register_user().await;
    let (status, _) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/servers"), &outsider_token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore]
async fn owner_can_remove_a_server() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (_, server) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/servers"), &owner_token, json!({ "name": "S", "host": "h" })).await;
    let server_id = server["id"].as_str().unwrap();

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}/servers/{server_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, servers) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/servers"), &owner_token).await;
    assert_eq!(servers.as_array().unwrap().len(), 0);
}

#[tokio::test]
#[ignore]
async fn an_empty_host_is_rejected() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (status, body) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/servers"), &owner_token, json!({ "name": "S", "host": "  " })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}
