//! The boundary between "what VibeSSH wants to ask" and "how a particular
//! vendor's HTTP API wants to be asked".
//!
//! Everything above this line - context collection, redaction, the system
//! prompt, the service that orchestrates them - is written against
//! `AiProvider` and knows nothing about OpenAI's request shape. That is
//! what makes the first implementation replaceable rather than load-bearing,
//! and it is what lets the tests drive the whole service with a mock and no
//! network at all, which the brief for this feature required.

use crate::errors::AppResult;

/// Wire-neutral message roles. Distinct from `models::AiRole`, which has no
/// `System` variant on purpose: a system message is something this app
/// constructs, never something the frontend can submit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

/// One request, as this app thinks about it.
#[derive(Debug, Clone)]
pub struct AiRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// A ceiling on the answer, not on the request. Present because a
    /// runaway generation against a metered provider is the user's money,
    /// and because the answer format this assistant is asked for - three
    /// short points - does not need thousands of tokens.
    pub max_tokens: Option<u32>,
    /// Low by default (see `ai_service`): this assistant is asked to reason
    /// from a supplied snapshot and say when it cannot, which is the
    /// opposite of the job temperature helps with.
    pub temperature: Option<f32>,
}

/// What VibeSSH needs from a model endpoint. Two methods, because the app
/// genuinely has two different needs.
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    /// The whole answer, once. Used by Test connection, where there is
    /// nothing to show progressively and a single round trip is the clearer
    /// signal.
    async fn complete(&self, request: &AiRequest) -> AppResult<String>;

    /// The same answer, delivered as it is generated. `on_delta` is called
    /// with each fragment in order; the assembled text is still returned, so
    /// a caller that also wants the whole thing does not have to accumulate
    /// it a second time.
    ///
    /// The default implementation is a real, correct one: it completes
    /// normally and hands the answer over in a single delta. A provider
    /// whose endpoint does not support streaming - or a test that does not
    /// care - therefore gets working behaviour rather than an error, which
    /// is the "non-streaming first, streaming where it is available" shape
    /// this feature was asked for.
    async fn stream(&self, request: &AiRequest, on_delta: &(dyn Fn(String) + Send + Sync)) -> AppResult<String> {
        let answer = self.complete(request).await?;
        if !answer.is_empty() {
            on_delta(answer.clone());
        }
        Ok(answer)
    }
}
