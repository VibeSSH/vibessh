//! Real integration tests for Teams + Team Members, against a real
//! Postgres. Every test registers its own fresh user(s) via
//! common::register_user rather than sharing fixtures, so tests never
//! interfere with each other even run in parallel against the same
//! persistent dev database.
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

use common::{delete, get_with_bearer, post, post_with_bearer, register_user, test_router};

#[tokio::test]
#[ignore]
async fn creating_a_team_makes_the_creator_its_owner_and_only_member() {
    let (_, access_token) = register_user().await;

    let (status, team) = post_with_bearer(test_router().await, "/teams", &access_token, json!({ "name": "Acme Ops" })).await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    assert_eq!(team["name"], "Acme Ops");
    let team_id = team["id"].as_str().unwrap();

    let (status, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &access_token).await;
    assert_eq!(status, StatusCode::OK);
    let members = members.as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["isOwner"], true);
}

#[tokio::test]
#[ignore]
async fn list_teams_only_returns_teams_the_caller_is_a_member_of() {
    let (_, access_token) = register_user().await;
    let (status, _) = post_with_bearer(test_router().await, "/teams", &access_token, json!({ "name": "My Team" })).await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, other_access_token) = register_user().await;

    let (status, mine) = get_with_bearer(test_router().await, "/teams", &access_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mine.as_array().unwrap().len(), 1);

    let (status, theirs) = get_with_bearer(test_router().await, "/teams", &other_access_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(theirs.as_array().unwrap().len(), 0);
}

#[tokio::test]
#[ignore]
async fn a_non_member_gets_not_found_not_forbidden_for_a_real_team() {
    // Deliberately 404, not 403 - a non-member shouldn't be able to tell a
    // real team they're excluded from apart from one that doesn't exist.
    let (_, owner_token) = register_user().await;
    let (_, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Private Team" })).await;
    let team_id = team["id"].as_str().unwrap();

    let (_, outsider_token) = register_user().await;
    let (status, _) = get_with_bearer(test_router().await, &format!("/teams/{team_id}"), &outsider_token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore]
async fn owner_can_add_an_existing_user_by_email_and_they_appear_as_a_non_owner_member() {
    let (_, owner_token) = register_user().await;
    let (_, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Growing Team" })).await;
    let team_id = team["id"].as_str().unwrap();

    let (new_member_email, _) = register_user().await;
    let (status, _) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members"),
        &owner_token,
        json!({ "email": new_member_email }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    let members = members.as_array().unwrap();
    assert_eq!(members.len(), 2);
    let added = members.iter().find(|m| m["email"] == new_member_email).expect("new member should be listed");
    assert_eq!(added["isOwner"], false);
}

#[tokio::test]
#[ignore]
async fn adding_a_member_who_is_already_on_the_team_is_a_conflict() {
    let (_, owner_token) = register_user().await;
    let (_, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Team" })).await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, _) = register_user().await;
    let add = || json!({ "email": member_email.clone() });
    let (status, _) = post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, add()).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, add()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
#[ignore]
async fn adding_a_member_with_no_matching_account_is_not_found() {
    let (_, owner_token) = register_user().await;
    let (_, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Team" })).await;
    let team_id = team["id"].as_str().unwrap();

    let (status, body) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members"),
        &owner_token,
        json!({ "email": "nobody-registered-with-this-address@example.com" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
#[ignore]
async fn a_non_owner_member_cannot_add_other_members() {
    let (_, owner_token) = register_user().await;
    let (_, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Team" })).await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, member_token) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;

    let (someone_elses_email, _) = register_user().await;
    let (status, body) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/members"),
        &member_token,
        json!({ "email": someone_elses_email }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
#[ignore]
async fn owner_can_remove_a_non_owner_member() {
    let (_, owner_token) = register_user().await;
    let (_, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Team" })).await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, _) = register_user().await;
    let (_, member) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;
    let _ = member;

    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    let member_user_id = members.as_array().unwrap().iter().find(|m| m["email"] == member_email).unwrap()["userId"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}/members/{member_user_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, members_after) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    assert_eq!(members_after.as_array().unwrap().len(), 1);
}

#[tokio::test]
#[ignore]
async fn the_owner_cannot_be_removed_as_a_member() {
    let (_, owner_token) = register_user().await;
    let (_, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Team" })).await;
    let team_id = team["id"].as_str().unwrap();
    let owner_id = team["ownerId"].as_str().unwrap();

    let (status, body) = delete(test_router().await, &format!("/teams/{team_id}/members/{owner_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
#[ignore]
async fn a_non_owner_member_cannot_remove_anyone() {
    let (_, owner_token) = register_user().await;
    let (_, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Team" })).await;
    let team_id = team["id"].as_str().unwrap();
    let owner_id = team["ownerId"].as_str().unwrap();

    let (member_email, member_token) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}/members/{owner_id}"), &member_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
#[ignore]
async fn owner_can_delete_the_team_and_it_becomes_unreachable_afterward() {
    let (_, owner_token) = register_user().await;
    let (_, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Doomed Team" })).await;
    let team_id = team["id"].as_str().unwrap();

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = get_with_bearer(test_router().await, &format!("/teams/{team_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore]
async fn a_non_owner_member_cannot_delete_the_team() {
    let (_, owner_token) = register_user().await;
    let (_, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Team" })).await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, member_token) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}"), &member_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
#[ignore]
async fn creating_a_team_without_authentication_is_unauthorized() {
    let (status, _) = post(test_router().await, "/teams", json!({ "name": "No Auth Team" })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
