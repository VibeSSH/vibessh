//! The hosted Vibe AI assistant's daily allowance, against a real Postgres.
//!
//! This file exists for one property that unit tests cannot reach: that N
//! requests arriving *together* against a limit of L produce exactly L
//! successes. The limit is enforced by a single SQL statement whose whole
//! purpose is that a read-then-write would let two concurrent requests both
//! see the same count and both pass - and a test that fires requests one
//! after another proves nothing about that, because a sequential test passes
//! against the broken implementation too.
//!
//! The upstream provider is a real HTTP server started in-process on an
//! ephemeral port, not a mock object. `ai::chat` reserves the allowance,
//! calls the provider over the network, and refunds on failure; stubbing
//! that call out would skip the refund path, which is the second thing worth
//! testing here.
//!
//! `#[ignore]` for the same reason as every other file in this directory:
//! these need a reachable Postgres named by `DATABASE_URL`, which a plain
//! checkout does not have, and cargo stops at the first failing test binary.
//! Run them explicitly:
//!
//! ```bash
//! DATABASE_URL=postgres://... cargo test -p vibessh-backend --test ai_quota -- --ignored --test-threads=1
//! ```
//!
//! `--test-threads=1` is not optional. `Upstream::from_env` reads process
//! environment at request time, these tests need different limits, and
//! environment is per process rather than per thread. The mutex below
//! serialises them anyway; the flag makes a failure legible rather than
//! turning it into a deadlock.

mod common;

use axum::extract::Json as ExtractJson;
use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use std::sync::Arc;

use tokio::sync::{Barrier, Mutex};

/// Serialises the tests in this binary - see the module comment.
///
/// Async-aware rather than a `std::sync::Mutex`: the guard is held across
/// awaits for the whole length of a test, which is exactly what a blocking
/// mutex must never do inside a runtime.
static ENV_LOCK: Mutex<()> = Mutex::const_new(());

/// Starts a throwaway OpenAI-compatible endpoint and returns its base URL.
///
/// Bound to port 0 so the OS picks a free one: a fixed port would make two
/// test binaries running at once fail for a reason that has nothing to do
/// with what they are testing.
async fn spawn_upstream(status: StatusCode, body: Value) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("binding a mock upstream");
    let addr = listener.local_addr().expect("the mock upstream's address");

    let app = Router::new().route(
        "/chat/completions",
        post(move |ExtractJson(_request): ExtractJson<Value>| {
            let body = body.clone();
            async move { (status, axum::Json(body)) }
        }),
    );

    tokio::spawn(async move {
        // Ends when the test process does; nothing here needs a graceful
        // shutdown, and awaiting one would just be a second thing to get
        // wrong in a test fixture.
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}")
}

/// What a well-behaved provider returns.
fn canned_answer() -> Value {
    json!({ "choices": [{ "message": { "role": "assistant", "content": "The port is taken." } }] })
}

/// Points the backend at a given upstream with a given daily limit.
///
/// Deliberately not a fixture that also cleans up: every test in this file
/// sets all four, so a leftover from one cannot survive into another.
fn configure(base_url: &str, daily_limit: i32) {
    std::env::set_var("AI_UPSTREAM_BASE_URL", base_url);
    std::env::set_var("AI_UPSTREAM_API_KEY", "test-key-never-leaves-this-process");
    std::env::set_var("AI_UPSTREAM_MODEL", "test-model");
    std::env::set_var("AI_DAILY_QUESTION_LIMIT", daily_limit.to_string());
}

fn a_question() -> Value {
    json!({ "messages": [{ "role": "user", "content": "why is it down?" }] })
}

/// Reads the stored counter directly, so the assertion does not depend on
/// the same endpoint it is checking.
async fn stored_count(email: &str) -> i32 {
    let db = common::test_db().await;
    sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(u.question_count, 0)
           FROM users AS usr
           LEFT JOIN ai_usage AS u ON u.user_id = usr.id AND u.usage_date = CURRENT_DATE
          WHERE usr.email = $1",
    )
    .bind(email)
    .fetch_one(&db)
    .await
    .expect("reading the stored usage count")
}

/// The property the single SQL statement exists for.
///
/// Twelve requests fired together against a limit of four. A read-then-write
/// implementation passes a sequential version of this test and fails this
/// one, which is the entire reason it is written with `tokio::spawn` rather
/// than a loop.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore]
async fn concurrent_questions_never_exceed_the_daily_limit() {
    let _guard = ENV_LOCK.lock().await;
    const LIMIT: i32 = 4;
    const ATTEMPTS: usize = 16;

    let upstream = spawn_upstream(StatusCode::OK, canned_answer()).await;
    configure(&upstream, LIMIT);

    let (email, token) = common::register_user().await;
    let router = common::test_router().await;

    // The barrier is what makes this a race rather than a fast loop.
    //
    // Spawning in a loop and letting each task start when it is scheduled
    // was measured against a deliberately broken (read-then-write)
    // implementation and caught it only one run in three: the early tasks
    // finished before the later ones were spawned, so most requests never
    // overlapped at all. Holding every task at the barrier and releasing
    // them together took that to every run.
    let barrier = Arc::new(Barrier::new(ATTEMPTS));

    let mut tasks = Vec::with_capacity(ATTEMPTS);
    for _ in 0..ATTEMPTS {
        let router = router.clone();
        let token = token.clone();
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            let (status, _body) = common::post_with_bearer(router, "/ai/chat", &token, a_question()).await;
            status
        }));
    }

    let mut allowed = 0;
    let mut refused = 0;
    let mut other = Vec::new();
    for task in tasks {
        match task.await.expect("a request task panicked") {
            StatusCode::OK => allowed += 1,
            StatusCode::TOO_MANY_REQUESTS => refused += 1,
            status => other.push(status),
        }
    }

    assert!(other.is_empty(), "unexpected statuses: {other:?}");
    assert_eq!(allowed, LIMIT as usize, "exactly {LIMIT} of {ATTEMPTS} concurrent questions should have been allowed");
    assert_eq!(refused, ATTEMPTS - LIMIT as usize);
    assert_eq!(stored_count(&email).await, LIMIT, "the stored counter must match what was allowed");
}

/// Going over the limit must not keep incrementing.
///
/// `ON CONFLICT ... DO UPDATE ... WHERE` returns no row when the limit is
/// reached, and the row is left alone. If the update ran regardless, a user
/// who kept retrying would drive the count arbitrarily high and the counter
/// would stop meaning anything - including for the "N left today" the
/// interface shows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn refused_questions_do_not_drive_the_counter_higher() {
    let _guard = ENV_LOCK.lock().await;
    const LIMIT: i32 = 2;

    let upstream = spawn_upstream(StatusCode::OK, canned_answer()).await;
    configure(&upstream, LIMIT);

    let (email, token) = common::register_user().await;
    let router = common::test_router().await;

    for _ in 0..LIMIT {
        let (status, body) = common::post_with_bearer(router.clone(), "/ai/chat", &token, a_question()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    for _ in 0..5 {
        let (status, _) = common::post_with_bearer(router.clone(), "/ai/chat", &token, a_question()).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    assert_eq!(stored_count(&email).await, LIMIT, "refusals must not increment the counter");
}

/// A provider outage must not cost the user a question.
///
/// The allowance is reserved before the upstream call, which is what makes
/// the concurrency test above work. The refund is the other half of that
/// bargain: without it, an upstream having a bad hour would quietly consume
/// somebody's whole day.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn a_failed_upstream_call_refunds_the_question() {
    let _guard = ENV_LOCK.lock().await;

    let upstream = spawn_upstream(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "upstream is having a bad day" })).await;
    configure(&upstream, 20);

    let (email, token) = common::register_user().await;
    let router = common::test_router().await;

    let (status, _body) = common::post_with_bearer(router, "/ai/chat", &token, a_question()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(stored_count(&email).await, 0, "a failed upstream call must give the question back");
}

/// The counter the interface shows has to be the one being enforced.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn the_quota_endpoint_reports_what_was_actually_spent() {
    let _guard = ENV_LOCK.lock().await;

    let upstream = spawn_upstream(StatusCode::OK, canned_answer()).await;
    configure(&upstream, 7);

    let (_email, token) = common::register_user().await;
    let router = common::test_router().await;

    let (status, body) = common::get_with_bearer(router.clone(), "/ai/quota", &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["used"], 0);
    assert_eq!(body["limit"], 7);
    assert!(body["resetsAt"].is_string(), "the UI needs a reset moment to show: {body}");

    common::post_with_bearer(router.clone(), "/ai/chat", &token, a_question()).await;

    let (_status, body) = common::get_with_bearer(router, "/ai/quota", &token).await;
    assert_eq!(body["used"], 1);
}

/// The allowance is per account, so one account running out must not affect
/// another. Worth asserting because the atomic statement keys on `user_id`,
/// and a mistake there would be invisible in every single-user test.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn one_exhausted_account_does_not_affect_another() {
    let _guard = ENV_LOCK.lock().await;

    let upstream = spawn_upstream(StatusCode::OK, canned_answer()).await;
    configure(&upstream, 1);

    let (_first_email, first) = common::register_user().await;
    let (_second_email, second) = common::register_user().await;
    let router = common::test_router().await;

    let (status, _) = common::post_with_bearer(router.clone(), "/ai/chat", &first, a_question()).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = common::post_with_bearer(router.clone(), "/ai/chat", &first, a_question()).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "the first account should be out");

    let (status, body) = common::post_with_bearer(router, "/ai/chat", &second, a_question()).await;
    assert_eq!(status, StatusCode::OK, "the second account has its own allowance: {body}");
}

/// A prompt large enough to be worth real money is refused before it is
/// forwarded, and refused without spending a question - the request never
/// happened, so charging for it would be wrong.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn an_oversized_prompt_is_refused_without_spending_the_allowance() {
    let _guard = ENV_LOCK.lock().await;

    let upstream = spawn_upstream(StatusCode::OK, canned_answer()).await;
    configure(&upstream, 20);

    let (email, token) = common::register_user().await;
    let router = common::test_router().await;

    let huge = json!({ "messages": [{ "role": "user", "content": "x".repeat(70_000) }] });
    let (status, _body) = common::post_with_bearer(router, "/ai/chat", &token, huge).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(stored_count(&email).await, 0, "a request that was never forwarded must not cost a question");
}

/// Anonymous callers must not reach the included model at all: the
/// allowance is per account, so a request with no account has nothing to
/// spend and would be an open proxy onto VibeSSH's key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn the_hosted_endpoint_requires_an_account() {
    let _guard = ENV_LOCK.lock().await;

    let upstream = spawn_upstream(StatusCode::OK, canned_answer()).await;
    configure(&upstream, 20);

    let (status, _body) = common::post(common::test_router().await, "/ai/chat", a_question()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
