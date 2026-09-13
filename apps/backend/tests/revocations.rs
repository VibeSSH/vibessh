//! What a team is still owed on its Nodes after somebody is removed.
//!
//! `#[ignore]` - every test here needs a reachable Postgres named by
//! `DATABASE_URL` (see `backend/.env.example`), the same convention the rest
//! of `apps/backend/tests/` uses. Run them explicitly:
//! `DATABASE_URL=... cargo test -p vibessh-backend -- --ignored`.
//!
//! The thing worth protecting here is the gap. Removing a member is one
//! statement in this backend; their account is a file on a machine it cannot
//! reach. Every test below is about that gap being recorded honestly and
//! only closed by something that actually went and closed it.
mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

use common::{delete, get_with_bearer, post_with_bearer, register_user, test_router};

async fn create_team(owner_token: &str, name: &str) -> Value {
    let (status, team) = post_with_bearer(test_router().await, "/teams", owner_token, json!({ "name": name })).await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    team
}

async fn add_server(owner_token: &str, team_id: &str, name: &str, host: &str) -> Value {
    let (status, server) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/servers"),
        owner_token,
        json!({ "name": name, "host": host, "sshPort": 22, "username": "root" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{server}");
    server
}

/// Adds a registered user to the team and returns their id and token.
async fn add_member(owner_token: &str, team_id: &str) -> (String, String, String) {
    let (email, token) = register_user().await;
    let (status, body) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), owner_token, json!({ "email": email })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (_, members) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), owner_token).await;
    let id = members.as_array().unwrap().iter().find(|m| m["email"] == email).unwrap()["userId"].as_str().unwrap().to_string();
    (id, email, token)
}

async fn pending(owner_token: &str, team_id: &str) -> Vec<Value> {
    let (status, body) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/revocations"), owner_token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().clone()
}

/// The whole point of the table: removing somebody here does not remove them
/// from the machines, so what is left over is written down per machine.
#[tokio::test]
#[ignore]
async fn removing_a_member_leaves_one_revocation_owed_per_shared_node() {
    let (_, owner_token) = register_user().await;
    let team_id = create_team(&owner_token, "Revocations").await["id"].as_str().unwrap().to_string();
    add_server(&owner_token, &team_id, "Prod", "10.0.0.1").await;
    add_server(&owner_token, &team_id, "Staging", "10.0.0.2").await;
    let (member_id, member_email, _) = add_member(&owner_token, &team_id).await;

    assert!(pending(&owner_token, &team_id).await.is_empty(), "nothing is owed before anybody is removed");

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}/members/{member_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let owed = pending(&owner_token, &team_id).await;
    assert_eq!(owed.len(), 2, "one per shared Node: {owed:?}");
    for row in &owed {
        assert_eq!(row["email"], member_email);
        // The account to remove, and which machine still has it - "pending"
        // without an address is an alarm nobody can act on.
        assert!(row["nodeUsername"].as_str().unwrap().starts_with("vibessh-m-"), "{row}");
        assert!(row["host"].as_str().is_some(), "{row}");
        assert!(row["serverName"].as_str().is_some(), "{row}");
    }
    let mut hosts: Vec<&str> = owed.iter().map(|row| row["host"].as_str().unwrap()).collect();
    hosts.sort_unstable();
    assert_eq!(hosts, ["10.0.0.1", "10.0.0.2"]);
}

/// A team with no shared Nodes owes nothing, and that is not an error.
#[tokio::test]
#[ignore]
async fn removing_a_member_from_a_team_with_no_nodes_owes_nothing() {
    let (_, owner_token) = register_user().await;
    let team_id = create_team(&owner_token, "No nodes").await["id"].as_str().unwrap().to_string();
    let (member_id, _, _) = add_member(&owner_token, &team_id).await;

    let (status, _) = delete(test_router().await, &format!("/teams/{team_id}/members/{member_id}"), &owner_token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(pending(&owner_token, &team_id).await.is_empty());
}

/// Completing is what an install says after the Node did it. Doing it twice
/// is not two events - the second install to try gets told it is already
/// done rather than writing a second audit entry for one removal.
#[tokio::test]
#[ignore]
async fn a_revocation_can_only_be_completed_once() {
    let (_, owner_token) = register_user().await;
    let team_id = create_team(&owner_token, "Complete once").await["id"].as_str().unwrap().to_string();
    add_server(&owner_token, &team_id, "Prod", "10.0.0.3").await;
    let (member_id, _, _) = add_member(&owner_token, &team_id).await;
    delete(test_router().await, &format!("/teams/{team_id}/members/{member_id}"), &owner_token).await;

    let owed = pending(&owner_token, &team_id).await;
    let revocation_id = owed[0]["id"].as_str().unwrap().to_string();

    let (status, body) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/revocations/{revocation_id}"), &owner_token, json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(pending(&owner_token, &team_id).await.is_empty(), "completing it takes it off the list");

    let (status, body) =
        post_with_bearer(test_router().await, &format!("/teams/{team_id}/revocations/{revocation_id}"), &owner_token, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "revocation_not_pending", "{body}");
}

/// Bringing somebody back cancels a removal nobody had carried out yet.
///
/// The dangerous version of this is not an extra row: it is a sync that
/// grants a current member their account and then removes it again in the
/// same pass, because the record still said to. Removing them afterwards is
/// a new and real thing owed.
#[tokio::test]
#[ignore]
async fn re_adding_somebody_cancels_what_was_owed_for_them() {
    let (_, owner_token) = register_user().await;
    let team_id = create_team(&owner_token, "Churn").await["id"].as_str().unwrap().to_string();
    add_server(&owner_token, &team_id, "Prod", "10.0.0.4").await;
    let (member_id, member_email, _) = add_member(&owner_token, &team_id).await;

    delete(test_router().await, &format!("/teams/{team_id}/members/{member_id}"), &owner_token).await;
    assert_eq!(pending(&owner_token, &team_id).await.len(), 1);

    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;
    assert!(
        pending(&owner_token, &team_id).await.is_empty(),
        "a member who is back is not somebody whose account should be taken away",
    );

    delete(test_router().await, &format!("/teams/{team_id}/members/{member_id}"), &owner_token).await;
    assert_eq!(pending(&owner_token, &team_id).await.len(), 1, "removing them again is owed again");
}

/// Two removals with no sync in between are one account to take off the
/// machine, not two.
#[tokio::test]
#[ignore]
async fn what_is_owed_does_not_stack_and_comes_back_after_it_lands() {
    let (_, owner_token) = register_user().await;
    let team_id = create_team(&owner_token, "Stacking").await["id"].as_str().unwrap().to_string();
    add_server(&owner_token, &team_id, "Prod", "10.0.0.5").await;
    let (member_id, member_email, _) = add_member(&owner_token, &team_id).await;

    delete(test_router().await, &format!("/teams/{team_id}/members/{member_id}"), &owner_token).await;
    let owed = pending(&owner_token, &team_id).await;
    assert_eq!(owed.len(), 1);
    let revocation_id = owed[0]["id"].as_str().unwrap().to_string();
    post_with_bearer(test_router().await, &format!("/teams/{team_id}/revocations/{revocation_id}"), &owner_token, json!({})).await;

    post_with_bearer(test_router().await, &format!("/teams/{team_id}/members"), &owner_token, json!({ "email": member_email })).await;
    delete(test_router().await, &format!("/teams/{team_id}/members/{member_id}"), &owner_token).await;
    let owed = pending(&owner_token, &team_id).await;
    assert_eq!(owed.len(), 1, "the account is on the machine again, so it is owed again");
    assert_ne!(owed[0]["id"].as_str().unwrap(), revocation_id, "a new record, not the completed one reopened");
}

/// Reading what is owed needs only membership - the people who would notice
/// an overdue revocation are exactly the ones who must not be hidden from
/// it. Completing one is a claim about a machine, so it needs the permission
/// that manages the team's servers.
#[tokio::test]
#[ignore]
async fn a_member_can_read_what_is_owed_but_not_declare_it_done() {
    let (_, owner_token) = register_user().await;
    let team_id = create_team(&owner_token, "Who may say so").await["id"].as_str().unwrap().to_string();
    add_server(&owner_token, &team_id, "Prod", "10.0.0.6").await;
    let (removed_id, _, _) = add_member(&owner_token, &team_id).await;
    let (_, _, bystander_token) = add_member(&owner_token, &team_id).await;
    delete(test_router().await, &format!("/teams/{team_id}/members/{removed_id}"), &owner_token).await;

    let (status, owed) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/revocations"), &bystander_token).await;
    assert_eq!(status, StatusCode::OK, "{owed}");
    let revocation_id = owed.as_array().unwrap()[0]["id"].as_str().unwrap().to_string();

    let (status, body) = post_with_bearer(
        test_router().await,
        &format!("/teams/{team_id}/revocations/{revocation_id}"),
        &bystander_token,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(pending(&owner_token, &team_id).await.len(), 1, "and it is still owed");
}

/// Somebody outside the team sees nothing at all, not an empty list that
/// would tell them the team exists.
#[tokio::test]
#[ignore]
async fn an_outsider_cannot_read_what_a_team_is_owed() {
    let (_, owner_token) = register_user().await;
    let team_id = create_team(&owner_token, "Private").await["id"].as_str().unwrap().to_string();
    let (_, outsider_token) = register_user().await;

    let (status, _) = get_with_bearer(test_router().await, &format!("/teams/{team_id}/revocations"), &outsider_token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
