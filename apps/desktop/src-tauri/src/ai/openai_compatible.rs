//! `AiProvider` over the OpenAI `/chat/completions` API.
//!
//! One implementation covers OpenAI itself, OpenRouter, Groq, Together,
//! DeepInfra, LM Studio, llama.cpp's server and vLLM, because they all
//! speak the same request shape. That is the whole reason this is the first
//! provider rather than a vendor SDK: the user's own endpoint is as
//! supported as the hosted ones, and nothing here needs to know which they
//! picked.
//!
//! **Two rules this module exists to keep.**
//!
//! The API key appears in exactly one place - the `Authorization` header
//! built in `request_builder` - and is never logged, never returned in an
//! error, and never put in a URL or a query string.
//!
//! A provider's response body never reaches the user. It is third-party
//! text of unknown shape, it is where a chatty endpoint would echo the
//! request back (key included), and the brief for this feature was explicit
//! that a raw API error is not an error message. Bodies are truncated,
//! passed through the same redaction every other outbound string gets, and
//! written to the log; the user sees one of `errors::AppError`'s plain
//! sentences.

use std::time::Duration;

use futures_util::StreamExt;
use serde::Deserialize;

use crate::errors::{AppError, AppResult};

use super::provider::{AiProvider, AiRequest, ChatMessage, ChatRole};
use super::sanitizer;

/// How long to wait for the endpoint to answer at all.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a non-streaming request may take end to end. Generous, because
/// a large model behind a busy proxy is genuinely slow, and a wrong answer
/// here is a failure the user cannot distinguish from a broken endpoint.
const COMPLETE_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a *stream* may go without producing anything.
///
/// Deliberately not a total budget. A long answer legitimately takes
/// minutes, so capping the whole stream would kill exactly the requests the
/// user most wants; what actually indicates a dead endpoint is silence
/// between chunks. Applied per chunk in `stream`.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// How much of a failed response body is worth keeping for the log.
const MAX_LOGGED_BODY: usize = 600;

pub struct OpenAiCompatibleProvider {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: &str, api_key: &str) -> AppResult<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|err| {
                log::error!("couldn't build the AI HTTP client: {err}");
                AppError::AiProviderUnavailable
            })?;
        Ok(Self { base_url: base_url.trim().to_string(), api_key: api_key.to_string(), client })
    }

    fn request_builder(&self, body: serde_json::Value, timeout: Option<Duration>) -> reqwest::RequestBuilder {
        let mut builder = self.client.post(chat_completions_url(&self.base_url)).json(&body);
        // The one place the key is used. `bearer_auth` rather than a
        // hand-built header so there is no format string near it.
        //
        // Omitted entirely when there is no key, rather than sent empty: a
        // self-hosted endpoint (LM Studio, llama.cpp's server, a local vLLM)
        // needs no key at all, and several of them reject a bare
        // `Authorization: Bearer` with a 401 that would read to the user as
        // "your key is wrong" when they never had one.
        if !self.api_key.is_empty() {
            builder = builder.bearer_auth(&self.api_key);
        }
        if let Some(timeout) = timeout {
            builder = builder.timeout(timeout);
        }
        builder
    }
}

/// Joins the configured base onto the endpoint path.
///
/// Three inputs have to work, because all three are what people actually
/// paste out of a provider's documentation page:
/// `https://openrouter.ai/api/v1`, the same with a trailing slash, and the
/// full `https://openrouter.ai/api/v1/chat/completions`. Rejecting the
/// third would be technically defensible and would read, to the user, as
/// the feature being broken.
pub fn chat_completions_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

fn role_name(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
    }
}

fn messages_json(messages: &[ChatMessage]) -> serde_json::Value {
    serde_json::Value::Array(
        messages
            .iter()
            .map(|message| serde_json::json!({ "role": role_name(message.role), "content": message.content }))
            .collect(),
    )
}

fn body_for(request: &AiRequest, stream: bool) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": request.model,
        "messages": messages_json(&request.messages),
        "stream": stream,
    });
    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    if let Some(temperature) = request.temperature {
        body["temperature"] = serde_json::json!(temperature);
    }
    body
}

/// Turns a transport-level failure into one of the four user-facing codes.
///
/// `is_timeout` is checked before anything else because reqwest reports a
/// timeout as a request error like any other, and "it took too long" is a
/// different thing for the user to do something about than "it could not be
/// reached".
fn transport_error(err: &reqwest::Error, operation: &'static str) -> AppError {
    if err.is_timeout() {
        log::warn!("the AI provider timed out during {operation}");
        return AppError::Timeout { operation: "the AI request", seconds: COMPLETE_TIMEOUT.as_secs() };
    }
    // `err` renders the URL but never the headers, so the bearer token
    // cannot reach this. The URL still goes through the sanitizer: the base
    // is whatever the user typed, and an endpoint that wants its key as a
    // query parameter would otherwise put it in the log. Naming the endpoint
    // at all is what makes a typo'd base URL diagnosable.
    log::warn!("couldn't reach the AI provider during {operation}: {}", sanitizer::sanitize_text(&err.to_string()));
    AppError::AiProviderUnavailable
}

/// Maps an HTTP status onto the code whose remedy matches.
///
/// 404 is the interesting one. On a hosted provider it usually means the
/// model name is wrong; on a self-hosted endpoint it usually means the base
/// URL is missing a path segment. Both are things the user fixes in the
/// same Settings card, and the message names the model, which is the more
/// common of the two by a wide margin.
fn status_error(status: reqwest::StatusCode, model: &str, body: &str) -> AppError {
    let logged: String = sanitizer::sanitize_text(body).chars().take(MAX_LOGGED_BODY).collect();
    log::warn!("the AI provider answered {status}: {logged}");
    match status.as_u16() {
        401 | 403 => AppError::AiAuthFailed,
        404 | 400 => AppError::AiModelUnavailable { model: model.to_string() },
        429 => AppError::AiRateLimited,
        _ => AppError::AiProviderUnavailable,
    }
}

#[derive(Deserialize)]
struct CompletionResponse {
    choices: Vec<CompletionChoice>,
}

#[derive(Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
}

#[derive(Deserialize)]
struct CompletionMessage {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
}

#[derive(Deserialize, Default)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
}

#[async_trait::async_trait]
impl AiProvider for OpenAiCompatibleProvider {
    async fn complete(&self, request: &AiRequest) -> AppResult<String> {
        let response = self
            .request_builder(body_for(request, false), Some(COMPLETE_TIMEOUT))
            .send()
            .await
            .map_err(|err| transport_error(&err, "a completion"))?;

        let status = response.status();
        let body = response.text().await.map_err(|err| transport_error(&err, "reading a completion"))?;
        if !status.is_success() {
            return Err(status_error(status, &request.model, &body));
        }

        let parsed: CompletionResponse = serde_json::from_str(&body).map_err(|err| {
            let logged: String = sanitizer::sanitize_text(&body).chars().take(MAX_LOGGED_BODY).collect();
            log::warn!("the AI provider's response didn't parse ({err}): {logged}");
            AppError::AiProviderUnavailable
        })?;

        match parsed.choices.into_iter().next() {
            Some(choice) => Ok(choice.message.content),
            // A 200 with no choices is well-formed JSON that answers
            // nothing. Reported as a provider failure rather than an empty
            // assistant message, so the user is not left staring at a blank
            // reply wondering whether that was the answer.
            None => {
                log::warn!("the AI provider returned no choices");
                Err(AppError::AiProviderUnavailable)
            }
        }
    }

    async fn stream(&self, request: &AiRequest, on_delta: &(dyn Fn(String) + Send + Sync)) -> AppResult<String> {
        let response = self
            .request_builder(body_for(request, true), None)
            .send()
            .await
            .map_err(|err| transport_error(&err, "a streamed completion"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(status_error(status, &request.model, &body));
        }

        let mut answer = String::new();
        let mut buffer = String::new();
        let mut stream = response.bytes_stream();

        loop {
            let next = match tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next()).await {
                Ok(next) => next,
                Err(_) => {
                    log::warn!("the AI provider stopped sending for {}s mid-answer", STREAM_IDLE_TIMEOUT.as_secs());
                    // Whatever arrived before the silence is kept rather
                    // than discarded: a partial answer the user can read is
                    // worth more than a clean error, and the UI marks the
                    // turn as interrupted either way.
                    if answer.is_empty() {
                        return Err(AppError::Timeout { operation: "the AI request", seconds: STREAM_IDLE_TIMEOUT.as_secs() });
                    }
                    return Ok(answer);
                }
            };
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|err| transport_error(&err, "reading a streamed completion"))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Server-sent events are newline-delimited, and a chunk
            // boundary lands wherever TCP put it - frequently mid-line, and
            // occasionally mid-JSON. Only whole lines are parsed; the
            // remainder stays in `buffer` for the next chunk to complete.
            while let Some(newline) = buffer.find('\n') {
                let line = buffer[..newline].trim().to_string();
                buffer.drain(..=newline);
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    return Ok(answer);
                }
                match serde_json::from_str::<StreamChunk>(data) {
                    Ok(parsed) => {
                        if let Some(delta) = parsed.choices.into_iter().next().and_then(|choice| choice.delta.content) {
                            if !delta.is_empty() {
                                answer.push_str(&delta);
                                on_delta(delta);
                            }
                        }
                    }
                    // One unparseable event is not a failed request -
                    // providers interleave keep-alives, usage records and
                    // vendor-specific frames in this channel. Dropping the
                    // frame and continuing is right; failing the turn over
                    // it would break streaming against half the endpoints
                    // this is meant to support.
                    Err(err) => log::debug!("skipping an unparseable stream event: {err}"),
                }
            }
        }

        Ok(answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_path_is_appended_once_however_the_base_was_pasted() {
        let expected = "https://openrouter.ai/api/v1/chat/completions";
        assert_eq!(chat_completions_url("https://openrouter.ai/api/v1"), expected);
        assert_eq!(chat_completions_url("https://openrouter.ai/api/v1/"), expected);
        assert_eq!(chat_completions_url("  https://openrouter.ai/api/v1  "), expected);
        assert_eq!(chat_completions_url("https://openrouter.ai/api/v1/chat/completions"), expected);
    }

    #[test]
    fn a_self_hosted_endpoint_with_a_port_is_joined_the_same_way() {
        assert_eq!(chat_completions_url("http://localhost:1234/v1"), "http://localhost:1234/v1/chat/completions");
    }

    #[test]
    fn the_request_body_carries_the_model_and_every_message_in_order() {
        let request = AiRequest {
            model: "meta-llama/llama-3.3-70b-instruct".to_string(),
            messages: vec![
                ChatMessage { role: ChatRole::System, content: "rules".to_string() },
                ChatMessage { role: ChatRole::User, content: "why is it down?".to_string() },
            ],
            max_tokens: Some(800),
            temperature: Some(0.2),
        };
        let body = body_for(&request, false);
        assert_eq!(body["model"], "meta-llama/llama-3.3-70b-instruct");
        assert_eq!(body["stream"], false);
        assert_eq!(body["max_tokens"], 800);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "why is it down?");
    }

    #[test]
    fn streaming_is_requested_only_when_asked_for() {
        let request = AiRequest { model: "m".into(), messages: vec![], max_tokens: None, temperature: None };
        assert_eq!(body_for(&request, true)["stream"], true);
        // Absent rather than null, so an endpoint that validates the field
        // strictly does not reject the request over a limit we did not set.
        assert!(body_for(&request, true).get("max_tokens").is_none());
    }

    #[test]
    fn each_provider_status_maps_to_the_error_whose_remedy_matches() {
        use reqwest::StatusCode;
        assert!(matches!(status_error(StatusCode::UNAUTHORIZED, "m", ""), AppError::AiAuthFailed));
        assert!(matches!(status_error(StatusCode::FORBIDDEN, "m", ""), AppError::AiAuthFailed));
        assert!(matches!(status_error(StatusCode::NOT_FOUND, "gpt-9", ""), AppError::AiModelUnavailable { .. }));
        assert!(matches!(status_error(StatusCode::TOO_MANY_REQUESTS, "m", ""), AppError::AiRateLimited));
        assert!(matches!(status_error(StatusCode::INTERNAL_SERVER_ERROR, "m", ""), AppError::AiProviderUnavailable));
        assert!(matches!(status_error(StatusCode::BAD_GATEWAY, "m", ""), AppError::AiProviderUnavailable));
    }

    /// The user-facing half of "never show a raw API error": whatever the
    /// provider said, the sentence the user reads is ours and mentions only
    /// the model they configured.
    #[test]
    fn a_provider_error_body_never_becomes_the_users_message() {
        let body = r#"{"error":{"message":"Incorrect API key provided: sk-abc123","type":"invalid_request_error"}}"#;
        let rendered = status_error(reqwest::StatusCode::UNAUTHORIZED, "gpt-4o-mini", body).to_string();
        assert!(!rendered.contains("sk-abc123"));
        assert!(!rendered.contains("invalid_request_error"));
        assert_eq!(rendered, "the AI provider rejected the API key");
    }

    #[test]
    fn a_malformed_completion_body_is_not_mistaken_for_an_answer() {
        assert!(serde_json::from_str::<CompletionResponse>("not json at all").is_err());
        // Well-formed JSON of the wrong shape has to fail too - this is the
        // case a naive `.get("choices")` walk would silently turn into an
        // empty answer.
        assert!(serde_json::from_str::<CompletionResponse>(r#"{"object":"error"}"#).is_err());
    }

    #[test]
    fn a_stream_event_yields_its_delta_and_a_keepalive_yields_nothing() {
        let chunk: StreamChunk = serde_json::from_str(r#"{"choices":[{"delta":{"content":"Hel"}}]}"#).unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hel"));

        // The frame that opens most streams: a role announcement with no
        // content. Must parse, and must contribute nothing to the answer.
        let opening: StreamChunk = serde_json::from_str(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#).unwrap();
        assert_eq!(opening.choices[0].delta.content, None);

        // A usage-only frame, which several providers send last.
        let usage: StreamChunk = serde_json::from_str(r#"{"choices":[],"usage":{"total_tokens":42}}"#).unwrap();
        assert!(usage.choices.is_empty());
    }
}
