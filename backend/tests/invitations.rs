//! Real integration tests for Invitations, against a real Postgres.
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
async fn owner_can_invite_an_email_and_the_raw_token_is_only_ever_returned_once() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (status, invitation) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/invitations"),
        &owner_token,
        json!({ "email": "invitee@example.com" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{invitation}");
    assert_eq!(invitation["email"], "invitee@example.com");
    assert_eq!(invitation["status"], "pending");
    assert!(invitation["token"].as_str().is_some());

    let (_, list) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/invitations"), &owner_token).await;
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 1);
    // The list view never includes the raw token again.
    assert!(list[0].get("token").is_none());
}

#[tokio::test]
async fn inviting_an_email_that_already_belongs_to_a_member_is_a_conflict() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, _) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;

    let (status, body) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/invitations"), &owner_token, json!({ "email": member_email })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn a_second_pending_invitation_to_the_same_email_is_a_conflict() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let invite = || json!({ "email": "repeat@example.com" });
    let (status, _) = post_with_bearer(test_router().await, &format!("/teams/{team_id}/invitations"), &owner_token, invite()).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = post_with_bearer(test_router().await, &format!("/teams/{team_id}/invitations"), &owner_token, invite()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn a_non_owner_member_cannot_send_invitations() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (member_email, member_token) = register_user().await;
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;

    let (status, _) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/invitations"),
        &member_token,
        json!({ "email": "someone@example.com" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_invited_user_can_accept_and_becomes_a_real_member() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (invitee_email, invitee_token) = register_user().await;
    let (_, invitation) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/invitations"), &owner_token, json!({ "email": invitee_email })).await;
    let token = invitation["token"].as_str().unwrap();

    let (status, joined_team) =
        post_with_bearer(test_router().await, &format!("/invitations/{token}/accept"), &invitee_token, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{joined_team}");
    assert_eq!(joined_team["id"], team["id"]);

    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    assert!(members.as_array().unwrap().iter().any(|m| m["email"] == invitee_email));

    let (_, invitations) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/invitations"), &owner_token).await;
    assert_eq!(invitations.as_array().unwrap()[0]["status"], "accepted");
}

#[tokio::test]
async fn accepting_an_invitation_with_a_role_assigns_that_role() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (_, role) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/roles"),
        &owner_token,
        json!({ "name": "Reviewer", "permissions": ["team.view"] }),
    )
    .await;
    let role_id = role["id"].as_str().unwrap();

    let (invitee_email, invitee_token) = register_user().await;
    let (_, invitation) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/invitations"),
        &owner_token,
        json!({ "email": invitee_email, "roleId": role_id }),
    )
    .await;
    let token = invitation["token"].as_str().unwrap();
    post_with_bearer(test_router().await, &format!("/invitations/{token}/accept"), &invitee_token, json!({})).await;

    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    let member_id = members.as_array().unwrap().iter().find(|m| m["email"] == invitee_email).unwrap()["userId"].as_str().unwrap();
    let (_, member_roles) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members/{member_id}/roles"), &owner_token).await;
    assert_eq!(member_roles.as_array().unwrap()[0]["name"], "Reviewer");
}

#[tokio::test]
async fn a_different_logged_in_user_cannot_accept_someone_elses_invitation() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (invitee_email, _) = register_user().await;
    let (_, invitation) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/invitations"), &owner_token, json!({ "email": invitee_email })).await;
    let token = invitation["token"].as_str().unwrap();

    let (_, someone_elses_token) = register_user().await;
    let (status, body) =
        post_with_bearer(test_router().await, &format!("/invitations/{token}/accept"), &someone_elses_token, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn an_unknown_token_is_not_found() {
    let (_, token) = register_user().await;
    let (status, _) = post_with_bearer(test_router().await, "/invitations/not-a-real-token/accept", &token, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_invitee_can_decline_instead_of_accepting() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (invitee_email, invitee_token) = register_user().await;
    let (_, invitation) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/invitations"), &owner_token, json!({ "email": invitee_email })).await;
    let token = invitation["token"].as_str().unwrap();

    let (status, _) = post_with_bearer(test_router().await, &format!("/invitations/{token}/decline"), &invitee_token, json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token).await;
    assert!(!members.as_array().unwrap().iter().any(|m| m["email"] == invitee_email));

    // A declined invitation can't be accepted afterward either.
    let (status, _) = post_with_bearer(test_router().await, &format!("/invitations/{token}/accept"), &invitee_token, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn owner_can_revoke_a_pending_invitation_and_it_can_no_longer_be_accepted() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (invitee_email, invitee_token) = register_user().await;
    let (_, invitation) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/invitations"), &owner_token, json!({ "email": invitee_email })).await;
    let invitation_id = invitation["id"].as_str().unwrap();
    let token = invitation["token"].as_str().unwrap().to_string();

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}/invitations/{invitation_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = post_with_bearer(test_router().await, &format!("/invitations/{token}/accept"), &invitee_token, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn revoking_an_already_accepted_invitation_is_a_conflict() {
    let (_, owner_token) = register_user().await;
    let team = create_team(&owner_token, "Team").await;
    let team_id = team["id"].as_str().unwrap();

    let (invitee_email, invitee_token) = register_user().await;
    let (_, invitation) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/invitations"), &owner_token, json!({ "email": invitee_email })).await;
    let invitation_id = invitation["id"].as_str().unwrap();
    let token = invitation["token"].as_str().unwrap();
    post_with_bearer(test_router().await, &format!("/invitations/{token}/accept"), &invitee_token, json!({})).await;

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}/invitations/{invitation_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::CONFLICT);
}
