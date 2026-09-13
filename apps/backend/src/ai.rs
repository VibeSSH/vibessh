//! The hosted Vibe AI assistant: VibeSSH's own model, on VibeSSH's own key,
//! with a per-account daily allowance.
//!
//! **Why this endpoint exists at all.** The desktop app can already talk to
//! any OpenAI-compatible endpoint with a key the user supplies, and that
//! path needs no server. This one is for the opposite arrangement - a model
//! included with VibeSSH, paid for by VibeSSH - and that arrangement has one
//! hard constraint: the key must never reach the client. A desktop binary is
//! inspectable, so a key shipped inside it is a published key, and no
//! client-side counter matters once someone holds it.
//!
//! So the key lives here, in this process's environment, and the client
//! never sees it or the upstream's address. What the client gets back is an
//! answer and a count.
//!
//! **The allowance is reserved before the upstream call, not after.** Two
//! requests arriving together must not both read "19 used" and both proceed;
//! the increment and the limit check are a single statement. A failed
//! upstream call refunds the reservation, because a provider outage costing
//! the user a question would be charging them for nothing.

use axum::extract::State;
use axum::response::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::AuthUser;
use crate::errors::{ApiError, Detail};
use crate::AppState;

/// How many questions an account may ask per UTC day when the deployment
/// does not say otherwise. Overridden by `AI_DAILY_QUESTION_LIMIT`.
const DEFAULT_DAILY_LIMIT: i32 = 20;

/// The largest prompt this will forward, in characters across all messages.
///
/// The allowance is counted in questions, and a question's cost is not
/// constant: a Diagnose turn carrying a Node's configuration and forty log
/// lines is worth many times a one-line question. Without a ceiling, twenty
/// questions is not a bounded cost - it is twenty times whatever the client
/// chose to send. 60k characters is comfortably above a full Diagnose
/// snapshot (`ai::context` caps its own summary at 12k) and far below
/// anything worth paying for by accident.
const MAX_PROMPT_CHARS: usize = 60_000;

/// A ceiling on the answer, so one question cannot generate indefinitely.
const MAX_ANSWER_TOKENS: u32 = 1200;

/// How long to wait for the upstream provider.
const UPSTREAM_TIMEOUT_SECS: u64 = 120;

/// Where the hosted assistant actually sends requests. Read from the
/// environment at request time rather than cached at startup so a
/// deployment can rotate the key by restarting nothing but its own secret
/// store.
struct Upstream {
    base_url: String,
    api_key: String,
    model: String,
    daily_limit: i32,
}

impl Upstream {
    /// `None` when this deployment has not configured a hosted model. That
    /// is a normal state for a self-hosted VibeSSH backend whose users all
    /// bring their own keys, so it is reported as a plain refusal rather
    /// than an internal error.
    fn from_env() -> Option<Self> {
        let base_url = std::env::var("AI_UPSTREAM_BASE_URL").ok()?;
        let api_key = std::env::var("AI_UPSTREAM_API_KEY").ok()?;
        let model = std::env::var("AI_UPSTREAM_MODEL").ok()?;
        if base_url.trim().is_empty() || api_key.trim().is_empty() || model.trim().is_empty() {
            return None;
        }
        let daily_limit = std::env::var("AI_DAILY_QUESTION_LIMIT")
            .ok()
            .and_then(|raw| raw.parse::<i32>().ok())
            .filter(|limit| *limit > 0)
            .unwrap_or(DEFAULT_DAILY_LIMIT);
        Some(Self { base_url: base_url.trim().to_string(), api_key, model: model.trim().to_string(), daily_limit })
    }

    /// Same joining rule the desktop provider uses, so an operator can paste
    /// the address from the provider's documentation page in any of the
    /// three forms it appears in.
    fn chat_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    }
}

#[derive(Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
}

/// Deliberately no `model` field, and no system prompt override. The model
/// is this deployment's choice, and the standing instruction is assembled on
/// the desktop side and arrives as the first message - a client that could
/// name its own model here would be spending VibeSSH's allowance on whatever
/// it liked.
#[derive(Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaView {
    pub used: i32,
    pub limit: i32,
    /// Midnight UTC, when `used` returns to zero.
    pub resets_at: chrono::DateTime<chrono::Utc>,
    /// Characters of prompt sent today, across every question.
    ///
    /// Surfaced because the allowance is counted in questions and a
    /// question's cost is not: a Diagnose turn carrying a Node's
    /// configuration and forty log lines is worth many times a one-line
    /// question. Showing both is what lets somebody see that they have
    /// "18 of 20 left" and have still sent most of the day's actual cost.
    pub prompt_chars: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    pub content: String,
    pub quota: QuotaView,
}

fn next_midnight_utc() -> chrono::DateTime<chrono::Utc> {
    let tomorrow = chrono::Utc::now().date_naive().succ_opt().unwrap_or(chrono::Utc::now().date_naive());
    tomorrow.and_hms_opt(0, 0, 0).map(|naive| naive.and_utc()).unwrap_or_else(chrono::Utc::now)
}

/// The account's usage today, without spending any of it.
pub async fn quota(State(state): State<AppState>, AuthUser(user_id): AuthUser) -> Result<Json<QuotaView>, ApiError> {
    let Some(upstream) = Upstream::from_env() else {
        return Err(ApiError::NotFound(Detail::new("ai_not_hosted", "this VibeSSH backend does not offer a hosted AI model")));
    };
    let row: Option<(i32, i64)> =
        sqlx::query_as("SELECT question_count, prompt_chars FROM ai_usage WHERE user_id = $1 AND usage_date = CURRENT_DATE")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|err| ApiError::Internal(format!("failed to read AI usage: {err}")))?;
    let (used, prompt_chars) = row.unwrap_or((0, 0));

    Ok(Json(QuotaView { used, limit: upstream.daily_limit, resets_at: next_midnight_utc(), prompt_chars }))
}

/// Takes one question off the account's daily allowance, atomically.
///
/// The single statement is the point. A read-then-write would let two
/// requests that arrive together both see the same count and both pass, so
/// the limit would be advisory under exactly the load it exists for.
///
/// `ON CONFLICT ... WHERE` returns no row when the limit is already reached,
/// and crucially does not increment in that case - so a client that keeps
/// retrying past its allowance is refused rather than driven further into
/// debt.
async fn reserve_question(state: &AppState, user_id: uuid::Uuid, limit: i32, prompt_chars: i64) -> Result<i32, ApiError> {
    let used: Option<i32> = sqlx::query_scalar(
        "INSERT INTO ai_usage (user_id, usage_date, question_count, prompt_chars)
         VALUES ($1, CURRENT_DATE, 1, $3)
         ON CONFLICT (user_id, usage_date) DO UPDATE
           SET question_count = ai_usage.question_count + 1,
               prompt_chars = ai_usage.prompt_chars + $3,
               updated_at = NOW()
           WHERE ai_usage.question_count < $2
         RETURNING question_count",
    )
    .bind(user_id)
    .bind(limit)
    .bind(prompt_chars)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| ApiError::Internal(format!("failed to record AI usage: {err}")))?;

    used.ok_or(ApiError::TooManyRequests(Detail::new("ai_daily_limit_used", format!("the daily limit of {limit} questions has been used")).with("limit", limit)))
}

/// Gives a reserved question back after the upstream call failed.
///
/// Never fails the request: the user already has an error to read, and
/// turning a provider outage into a second, different error would tell them
/// nothing useful. Logged instead, because a refund that silently does not
/// happen is an allowance quietly leaking away.
async fn refund_question(state: &AppState, user_id: uuid::Uuid, prompt_chars: i64) {
    let result = sqlx::query(
        "UPDATE ai_usage
            SET question_count = GREATEST(question_count - 1, 0),
                prompt_chars = GREATEST(prompt_chars - $2, 0),
                updated_at = NOW()
          WHERE user_id = $1 AND usage_date = CURRENT_DATE",
    )
    .bind(user_id)
    .bind(prompt_chars)
    .execute(&state.db)
    .await;

    if let Err(err) = result {
        log::warn!("couldn't refund an AI question for {user_id}: {err}");
    }
}

pub async fn chat(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ApiError> {
    let Some(upstream) = Upstream::from_env() else {
        return Err(ApiError::NotFound(Detail::new("ai_not_hosted", "this VibeSSH backend does not offer a hosted AI model")));
    };

    if request.messages.is_empty() {
        return Err(ApiError::InvalidInput(Detail::new("ai_empty_question", "there is nothing to ask")));
    }
    let prompt_chars: usize = request.messages.iter().map(|message| message.content.chars().count()).sum();
    if prompt_chars > MAX_PROMPT_CHARS {
        return Err(ApiError::InvalidInput(
            Detail::new(
                "ai_question_too_large",
                format!("the question is too large for the included model ({prompt_chars} characters, limit {MAX_PROMPT_CHARS})"),
            )
            .with("characters", prompt_chars)
            .with("limit", MAX_PROMPT_CHARS),
        ));
    }

    let used = reserve_question(&state, user_id, upstream.daily_limit, prompt_chars as i64).await?;

    match call_upstream(&upstream, &request.messages).await {
        Ok(content) => Ok(Json(ChatResponse {
            content,
            quota: QuotaView { used, limit: upstream.daily_limit, resets_at: next_midnight_utc(), prompt_chars: prompt_chars as i64 },
        })),
        Err(err) => {
            refund_question(&state, user_id, prompt_chars as i64).await;
            Err(err)
        }
    }
}

/// Calls the configured provider.
///
/// Nothing the provider says reaches the caller. Its body can carry the
/// upstream's own error text, its account details, and - for a provider that
/// echoes the request - the key itself. The client gets one of two plain
/// sentences; the detail goes to this server's log, where the operator can
/// see it and the user cannot.
async fn call_upstream(upstream: &Upstream, messages: &[ChatMessage]) -> Result<String, ApiError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(UPSTREAM_TIMEOUT_SECS))
        .build()
        .map_err(|err| ApiError::UpstreamFailure(format!("failed to build the AI HTTP client: {err}")))?;

    let body = json!({
        "model": upstream.model,
        "messages": messages,
        "max_tokens": MAX_ANSWER_TOKENS,
        "temperature": 0.2,
        "stream": false,
    });

    let response = client
        .post(upstream.chat_url())
        .bearer_auth(&upstream.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|err| ApiError::UpstreamFailure(format!("couldn't reach the AI provider: {err}")))?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        // Truncated: an upstream that answers with a megabyte of HTML should
        // not put a megabyte in the log.
        let excerpt: String = text.chars().take(600).collect();
        log::warn!("the AI provider answered {status}: {excerpt}");
        return Err(ApiError::UpstreamFailure(format!("the AI provider answered {status}")));
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| ApiError::UpstreamFailure(format!("the AI provider's response didn't parse: {err}")))?;
    answer_from_response(&parsed)
}

/// The answer out of a chat-completions response, or why there isn't one.
///
/// Separate from the HTTP call so the awkward shapes can be tested without a
/// server, which is the only way the empty-answer case below was ever going
/// to be covered.
fn answer_from_response(parsed: &serde_json::Value) -> Result<String, ApiError> {
    let content = parsed["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ApiError::UpstreamFailure("the AI provider returned no answer".to_string()))?;

    // An empty string is not an answer, and `as_str()` returns one happily.
    //
    // A reasoning model makes this reachable rather than theoretical: it
    // spends tokens on a `reasoning` field first, and when the budget runs
    // out there the response is a well-formed success whose `content` is
    // "". Passed through, that reaches the user as a blank panel, which
    // reads as the app being broken and says nothing about why. An upstream
    // failure at least names what happened.
    if content.trim().is_empty() {
        let finish = parsed["choices"][0]["finish_reason"].as_str().unwrap_or("unknown");
        log::warn!("the AI provider returned an empty answer (finish_reason: {finish})");
        return Err(ApiError::UpstreamFailure(format!("the AI provider returned an empty answer (finish_reason: {finish})")));
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_returned_when_there_is_one() {
        let parsed = serde_json::json!({ "choices": [ { "finish_reason": "stop", "message": { "content": "Brak MYSQL_ROOT_PASSWORD." } } ] });
        assert_eq!(answer_from_response(&parsed).unwrap(), "Brak MYSQL_ROOT_PASSWORD.");
    }

    /// A reasoning model that spends its whole budget thinking returns a
    /// well-formed success with nothing in it. Treating that as an answer
    /// puts a blank panel in front of the user, which reads as the app being
    /// broken and explains nothing.
    #[test]
    fn an_empty_answer_is_a_failure_rather_than_an_empty_answer() {
        let parsed = serde_json::json!({ "choices": [ { "finish_reason": "length", "message": { "content": "", "reasoning": "thinking..." } } ] });
        let err = answer_from_response(&parsed).unwrap_err();
        assert!(matches!(err, ApiError::UpstreamFailure(_)), "{err:?}");
        // The reason is carried, because "it failed" alone sends an operator
        // looking in the wrong place - the budget, not the provider.
        assert!(format!("{err}").contains("length"), "{err}");
    }

    #[test]
    fn whitespace_only_counts_as_empty() {
        let parsed = serde_json::json!({ "choices": [ { "finish_reason": "stop", "message": { "content": "   
  " } } ] });
        assert!(answer_from_response(&parsed).is_err());
    }

    #[test]
    fn the_endpoint_path_is_appended_once_however_the_base_was_configured() {
        let expected = "https://example.com/v1/chat/completions";
        for base in ["https://example.com/v1", "https://example.com/v1/", "https://example.com/v1/chat/completions"] {
            let upstream = Upstream {
                base_url: base.to_string(),
                api_key: "k".into(),
                model: "m".into(),
                daily_limit: 20,
            };
            assert_eq!(upstream.chat_url(), expected);
        }
    }

    #[test]
    fn the_reset_moment_is_the_next_midnight_and_is_always_in_the_future() {
        let resets = next_midnight_utc();
        assert!(resets > chrono::Utc::now());
        assert_eq!(resets.time(), chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    }
}
