//! Real integration test for `cloud_client::CloudClient`, against a real
//! running instance of `backend/` (see backend/.env.example for how to
//! start one locally) - not a mock, matching this crate's other
//! integration tests driving real protocol code against a real server.
//! Requires VIBESSH_TEST_BACKEND_URL to point at a reachable backend;
//! skips (rather than fails) if it isn't set, since most dev machines
//! won't have the cloud backend running just to build the desktop app.
use vibessh_lib::cloud_client::CloudClient;

fn backend_url() -> Option<String> {
    std::env::var("VIBESSH_TEST_BACKEND_URL").ok()
}

fn unique_email() -> String {
    format!("desktop-test-{}@example.com", uuid::Uuid::new_v4())
}

#[tokio::test]
async fn register_then_login_and_list_teams_round_trips_against_a_real_backend() {
    let Some(base_url) = backend_url() else {
        eprintln!("skipping: VIBESSH_TEST_BACKEND_URL not set");
        return;
    };
    let client = CloudClient::new(base_url);
    let email = unique_email();

    let registered = client.register(&email, "correct horse battery staple", "Desktop Test").await.expect("register should succeed");
    assert_eq!(registered.user.email, email);

    let logged_in = client.login(&email, "correct horse battery staple").await.expect("login should succeed");
    assert_eq!(logged_in.user.id, registered.user.id);

    let me = client.me(&logged_in.access_token).await.expect("me should succeed with a fresh access token");
    assert_eq!(me.email, email);

    let teams = client.list_teams(&logged_in.access_token).await.expect("list_teams should succeed even when empty");
    assert!(teams.is_empty());

    let team = client.create_team(&logged_in.access_token, "Desktop Test Team").await.expect("create_team should succeed");
    assert_eq!(team.name, "Desktop Test Team");
    assert_eq!(team.owner_id, registered.user.id);

    let fetched_team = client.get_team(&logged_in.access_token, team.id).await.expect("get_team should succeed");
    assert_eq!(fetched_team.name, team.name);

    let members = client.list_members(&logged_in.access_token, team.id).await.expect("list_members should succeed");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].email, email);
    assert!(members[0].is_owner);

    let refreshed = client.refresh(&logged_in.refresh_token).await.expect("refresh should succeed");
    assert_eq!(refreshed.user.id, registered.user.id);
    assert_ne!(refreshed.refresh_token, logged_in.refresh_token, "refresh should rotate the token");

    client.logout(&refreshed.refresh_token).await.expect("logout should succeed");
}

#[tokio::test]
async fn wrong_password_is_a_clean_error_not_a_panic() {
    let Some(base_url) = backend_url() else {
        eprintln!("skipping: VIBESSH_TEST_BACKEND_URL not set");
        return;
    };
    let client = CloudClient::new(base_url);
    let email = unique_email();
    client.register(&email, "correct horse battery staple", "Desktop Test").await.unwrap();

    let result = client.login(&email, "wrong password").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn an_unreachable_backend_is_a_connection_error_not_a_panic() {
    let client = CloudClient::new("http://127.0.0.1:1".to_string());
    let result = client.login("nobody@example.com", "whatever-password").await;
    assert!(result.is_err());
}
