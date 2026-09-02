//! `AiProvider` for the model VibeSSH includes.
//!
//! **What is deliberately absent here.** No base URL, no model name, no API
//! key. All three live on the VibeSSH backend (`backend/src/ai.rs`), and
//! that is the entire reason this provider exists rather than the desktop
//! simply shipping with a key preconfigured. A desktop binary is
//! inspectable: a key inside it is a published key, and once someone holds
//! it they do not need to defeat any limit - they can spend the shared
//! allowance directly against the provider, without VibeSSH in the path at
//! all.
//!
//! So the allowance is counted where the key is. This side sends a
//! conversation and receives an answer and a number.
//!
//! **Why the token is captured up front rather than looked up per call.**
//! `services::resolve_ai_provider` builds this immediately before the turn
//! is spawned, and refreshes the access token at that moment. A turn is
//! bounded by the backend's own upstream timeout, so a token valid when the
//! turn starts is valid when it ends - which lets this hold plain owned
//! values instead of a borrow of the session that would have to outlive a
//! spawned task.

use crate::cloud_client::CloudClient;
use crate::errors::{AppError, AppResult};
use crate::models::CloudAiQuota;

use super::provider::{AiProvider, AiRequest, ChatRole};

pub struct HostedProvider {
    client: CloudClient,
    access_token: String,
}

impl HostedProvider {
    pub fn new(backend_url: &str, access_token: &str) -> Self {
        Self { client: CloudClient::new(backend_url.to_string()), access_token: access_token.to_string() }
    }

    /// The account's usage today, without spending any of it.
    pub async fn quota(&self) -> AppResult<CloudAiQuota> {
        self.client.ai_quota(&self.access_token).await.map_err(hosted_error)
    }
}

/// Turns the backend's answer into something the interface can translate.
///
/// `/ai/chat` and `/ai/quota` have a small, known error surface, and every
/// case in it deserves a different sentence:
///
/// - 404 means this deployment has no `AI_UPSTREAM_*` configured;
/// - 401 means the cloud session expired, which routes to a login;
/// - 429 is the daily allowance, already its own code;
/// - 400 is a prompt too large, whose message names the limit;
/// - anything else is the upstream provider failing, and there is nothing
///   the user can do about it beyond using their own key.
///
/// That last bucket is why this exists. Without it a provider rejecting
/// VibeSSH's key surfaced as `internal error: cloud backend returned 500
/// Internal Server Error: an internal error occurred` - the untranslated
/// fallback, saying nothing three times over. The coarse codes have no
/// translations by design (they are the floor for errors nobody has
/// classified yet), so anything reaching them here is a classification this
/// module failed to do.
///
/// Deliberately *not* done in `CloudClient`: a 404 from `/teams/:id` means
/// a missing team, and remapping every status there would break every other
/// cloud call.
fn hosted_error(err: AppError) -> AppError {
    match err {
        AppError::NotFound(_) => AppError::AiHostedUnavailable,
        // Pass through the ones that are already right.
        unauthorized @ AppError::Unauthorized(_) => unauthorized,
        quota @ AppError::AiQuotaExhausted => quota,
        invalid @ AppError::InvalidInput(_) => invalid,
        other => {
            // The detail is the backend's, already logged there; this keeps
            // a local trace of which turn it belonged to.
            log::warn!("the hosted AI call failed: {other}");
            AppError::AiProviderUnavailable
        }
    }
}

fn role_name(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
    }
}

#[async_trait::async_trait]
impl AiProvider for HostedProvider {
    /// `request.model` is ignored, and that is the point: the model is the
    /// backend's choice. A client that could name one would be spending
    /// VibeSSH's allowance on whatever it liked, so the backend does not
    /// accept the field at all.
    async fn complete(&self, request: &AiRequest) -> AppResult<String> {
        let messages = serde_json::Value::Array(
            request
                .messages
                .iter()
                .map(|message| serde_json::json!({ "role": role_name(message.role), "content": message.content }))
                .collect(),
        );

        let answer = self.client.ai_chat(&self.access_token, &messages).await.map_err(hosted_error)?;
        if answer.content.trim().is_empty() {
            log::warn!("the hosted AI returned an empty answer");
            return Err(AppError::AiProviderUnavailable);
        }
        Ok(answer.content)
    }

    // `stream` is not overridden. The default implementation completes and
    // emits the answer in one delta, which is correct behaviour rather than
    // a stub - the hosted path is non-streaming end to end, because a
    // proxied stream would mean holding a backend connection open per turn
    // and forwarding server-sent events through two hops for a cosmetic
    // gain. The panel renders it the same way either way.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_map_to_the_names_the_backend_expects() {
        assert_eq!(role_name(ChatRole::System), "system");
        assert_eq!(role_name(ChatRole::User), "user");
        assert_eq!(role_name(ChatRole::Assistant), "assistant");
    }
}
