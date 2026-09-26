//! Per-Application permissions for team members, against a real Postgres.
//!
//! `#[ignore]` for the same reason as every other test here - see
//! `audit.rs`. Run them with
//! `DATABASE_URL=... cargo test -p vibessh-backend -- --ignored`.
mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

use common::{get_with_bearer, post_with_bearer, put, register_user, test_router};

/// A team with one member besides the owner and one shared Application.
/// Returns (owner token, member token, team id, member id, application id).
async fn team_with_a_shared_application() -> (String, String, String, String, String) {
    let (_, owner_token) = register_user().await;
    let (status, team) = post_with_bearer(test_router().await, "/teams", &owner_token, json!({ "name": "Subusers" })).await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    let team_id = team["id"].as_str().unwrap().to_string();

    let (member_email, member_token) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;
    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    let member_id = members.as_array().unwrap().iter().find(|m| m["email"] == member_email).unwrap()["userId"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, application) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/applications"),
        &owner_token,
        json!({
            "localId": uuid::Uuid::new_v4(),
            "name": "oneblock",
            "blueprintId": "paper",
            "runtimeType": "docker",
            "workingDirectory": "/srv/vibessh/oneblock",
        }),
    )
    .await;
    assert!(status.is_success(), "{application}");
    let application_id = application["id"].as_str().unwrap().to_string();

    (owner_token, member_token, team_id, member_id, application_id)
}

async fn members_of(token: &str, team_id: &str, application_id: &str) -> Vec<Value> {
    let (status, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/applications/{application_id}/members"), token).await;
    assert_eq!(status, StatusCode::OK, "{members}");
    members.as_array().unwrap().clone()
}

/// The subuser case: somebody who may restart one server and read its files,
/// set when they are added and changed afterwards, and kept sorted and
/// without repeats whatever order the client sent.
#[tokio::test]
#[ignore]
async fn a_member_is_given_permissions_on_one_application_and_they_can_be_changed() {
    let (owner_token, _, team_id, member_id, application_id) = team_with_a_shared_application().await;

    let (status, body) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/applications/{application_id}/members"),
        &owner_token,
        json!({ "userId": member_id, "permissions": ["applications.lifecycle"] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let members = members_of(&owner_token, &team_id, &application_id).await;
    assert_eq!(members[0]["permissions"], json!(["applications.lifecycle"]));

    let (status, body) = put(
        test_router().await,
        &format!("/teams/{team_id}/applications/{application_id}/members/{member_id}"),
        &owner_token,
        json!({ "permissions": ["applications.files.read", "applications.console", "applications.files.read"] }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let members = members_of(&owner_token, &team_id, &application_id).await;
    assert_eq!(members[0]["permissions"], json!(["applications.console", "applications.files.read"]));

    let (_, events) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/audit"), &owner_token).await;
    assert!(
        events.as_array().unwrap().iter().any(|event| event["action"] == "application.access_changed"),
        "the change is not in the audit log: {events}"
    );
}

/// A permission that reaches every Application, or is root on the Node,
/// cannot be promised for one Application - refused by name, and nothing
/// stored.
#[tokio::test]
#[ignore]
async fn a_permission_that_cannot_be_held_to_one_application_is_refused() {
    let (owner_token, _, team_id, member_id, application_id) = team_with_a_shared_application().await;

    for permission in ["applications.delete", "node.terminal", "applications.config", "not.a.permission"] {
        let (status, body) = post_with_bearer(
            test_router().await,
            &format!("/teams/{team_id}/applications/{application_id}/members"),
            &owner_token,
            json!({ "userId": member_id, "permissions": [permission] }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{permission}: {body}");
        assert_eq!(body["code"], "permission_not_application_scoped", "{permission}: {body}");
    }
    assert!(members_of(&owner_token, &team_id, &application_id).await.is_empty());
}

/// Being on an Application's list is not the power to change it: that needs
/// `applications.create`, the same as sharing it.
#[tokio::test]
#[ignore]
async fn a_member_without_the_sharing_permission_cannot_change_anybodys_permissions() {
    let (owner_token, member_token, team_id, member_id, application_id) = team_with_a_shared_application().await;
    post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/applications/{application_id}/members"),
        &owner_token,
        json!({ "userId": member_id }),
    )
    .await;

    let (status, body) = put(
        test_router().await,
        &format!("/teams/{team_id}/applications/{application_id}/members/{member_id}"),
        &member_token,
        json!({ "permissions": ["applications.lifecycle"] }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(members_of(&owner_token, &team_id, &application_id).await[0]["permissions"], json!([]));
}

/// What the provisioning install reads to write a member's sudo rules: every
/// Application they are on the list for, with what it needs to name that
/// Application's container and folder, and what they may do there.
#[tokio::test]
#[ignore]
async fn the_team_access_list_carries_each_members_per_application_grants() {
    let (owner_token, _, team_id, member_id, application_id) = team_with_a_shared_application().await;
    post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/applications/{application_id}/members"),
        &owner_token,
        json!({ "userId": member_id, "permissions": ["applications.lifecycle", "applications.files.read"] }),
    )
    .await;

    let (status, access) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/access"), &owner_token).await;
    assert_eq!(status, StatusCode::OK, "{access}");
    let member = access.as_array().unwrap().iter().find(|entry| entry["userId"] == member_id.as_str()).expect("the member is listed");
    let applications = member["applications"].as_array().expect("grants are listed");
    assert_eq!(applications.len(), 1, "{member}");
    assert_eq!(applications[0]["runtimeType"], "docker");
    assert_eq!(applications[0]["workingDirectory"], "/srv/vibessh/oneblock");
    assert_eq!(applications[0]["permissions"], json!(["applications.files.read", "applications.lifecycle"]));
    assert!(applications[0]["localId"].is_string());
}

/// Changing somebody who is not on the list is a clear refusal, not a grant
/// that quietly adds them.
#[tokio::test]
#[ignore]
async fn changing_permissions_for_somebody_not_on_the_list_is_refused() {
    let (owner_token, _, team_id, member_id, application_id) = team_with_a_shared_application().await;

    let (status, body) = put(
        test_router().await,
        &format!("/teams/{team_id}/applications/{application_id}/members/{member_id}"),
        &owner_token,
        json!({ "permissions": ["applications.lifecycle"] }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "application_member_not_added");
}
