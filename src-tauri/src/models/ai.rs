//! The Vibe AI assistant's domain types.
//!
//! Two things here are load-bearing and worth stating up front.
//!
//! **The API key is not in `AiConfig`.** It lives in the OS keyring, the
//! same place every other secret in this app lives, and it is never part of
//! anything returned to the frontend. `AiConfigView` carries a `has_api_key`
//! boolean instead, which is all the UI actually needs to decide between
//! "enter a key" and "a key is stored" - the same write-only shape
//! `BackupDestinationConfig`/`SetBackupDestinationInput` already established
//! for the S3 secret access key.
//!
//! **The model is not hardcoded anywhere.** `model` is a plain string the
//! user types, because the set of models an OpenAI-compatible endpoint
//! serves is that endpoint's business - OpenRouter alone has hundreds, a
//! self-hosted llama.cpp has one with whatever name its owner gave it, and
//! a fixed list here would be wrong within a month.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which wire protocol the configured endpoint speaks.
///
/// One variant today, and it is still an enum rather than an implied
/// constant: the config file it is serialised into is written now and read
/// by every later build, so the field has to exist from the start for a
/// second provider to be addable without a migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AiProviderKind {
    /// Anything speaking OpenAI's `/chat/completions` shape - the OpenAI
    /// API itself, OpenRouter, Groq, Together, LM Studio, llama.cpp's
    /// server, vLLM. The user supplies the address, the model and the key.
    #[default]
    OpenAiCompatible,
    /// The model VibeSSH includes, reached through the VibeSSH backend.
    ///
    /// No address, no model name and no key on this side: all three belong
    /// to the backend, which is what makes a shared key possible at all - a
    /// key shipped to a desktop binary is a published key. The account's
    /// daily allowance is enforced there too, for the same reason.
    ///
    /// Requires being signed in to a VibeSSH account, because the allowance
    /// is per account.
    VibeSshHosted,
}

/// The non-secret half of the assistant's configuration, persisted as
/// plain JSON in the app config dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    /// `false` (the default) means the assistant is off: no request is
    /// ever built, no context is ever collected, and the UI says so. Off is
    /// the default deliberately - this is the one feature in VibeSSH that
    /// sends anything about the user's infrastructure to a third party, so
    /// it does not start doing that because the app was updated.
    pub enabled: bool,
    pub provider: AiProviderKind,
    /// The API root, e.g. `https://openrouter.ai/api/v1` or
    /// `https://api.openai.com/v1`. Stored without a trailing slash and
    /// without the `/chat/completions` suffix; see
    /// `ai::openai_compatible::chat_completions_url` for how the path is
    /// joined, and why a base that already ends in `/chat/completions` is
    /// accepted too.
    pub base_url: String,
    pub model: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: AiProviderKind::OpenAiCompatible,
            // No default endpoint or model on purpose. Picking one would
            // be picking a vendor on the user's behalf, and an empty field
            // with a placeholder is honest about the fact that this needs
            // an account somewhere before it can work.
            base_url: String::new(),
            model: String::new(),
        }
    }
}

/// `AiConfig` plus what the UI needs to know about the stored key without
/// ever receiving it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfigView {
    #[serde(flatten)]
    pub config: AiConfig,
    /// Whether a key is present in the keyring. Not the key, not its
    /// length, not a masked prefix - a boolean is the entire useful signal
    /// and anything more is a disclosure with no benefit.
    pub has_api_key: bool,
}

/// What the Settings form submits.
///
/// `api_key` blank means "leave the stored key alone", the same convention
/// `SetBackupDestinationInput::secret_access_key` uses, because the
/// frontend never holds the real key and therefore cannot resend it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAiConfigInput {
    pub enabled: bool,
    pub provider: AiProviderKind,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiRole {
    User,
    Assistant,
}

/// One turn of the conversation. The system prompt is not one of these -
/// it is added by `ai::prompt` at request time and never travels through
/// the frontend, so nothing the user types can displace or rewrite it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMessage {
    pub role: AiRole,
    pub content: String,
}

/// How much the assistant is being asked to look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AiMode {
    /// Questions about VibeSSH itself. Documentation snippets are still
    /// attached, but nothing is read from the user's Nodes.
    #[default]
    Ask,
    /// The same, plus a collected snapshot of whichever Node or
    /// Application the user was looking at.
    Diagnose,
}

/// What the collected context is about. Absent in `Ask`, and absent in
/// `Diagnose` too if the user opened the panel from somewhere with no
/// subject - in which case the model is told the context is missing rather
/// than being left to invent one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AiContextRef {
    Application { id: Uuid },
    Node { id: Uuid },
}

/// One turn's worth of input from the frontend.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTurnRequest {
    pub mode: AiMode,
    #[serde(default)]
    pub context: Option<AiContextRef>,
    /// The whole conversation so far, ending with the message the user
    /// just typed. Sent in full each turn because nothing about the
    /// conversation is persisted on the Rust side - see
    /// `services::ai_service`'s own doc comment for why that is the right
    /// trade for this version.
    pub messages: Vec<AiMessage>,
}

/// The collected, sanitized context - both what gets embedded in the
/// prompt and what the UI shows the user before it is sent.
///
/// The preview is not a nicety. `AGENTS.md` §6 is explicit that a boundary
/// the interface does not show is not a boundary, and "which facts about my
/// server leave this machine" is exactly such a boundary. `summary` is
/// therefore the literal text that goes into the request, not a
/// paraphrase - if the two could drift, the preview would be reassurance
/// rather than disclosure.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiContextBundle {
    pub summary: String,
    /// Short labels for what was collected, for the preview's header.
    pub sources: Vec<String>,
    /// What could not be collected and why - an unreachable Node, a Docker
    /// daemon that did not answer. Included in the prompt as well as the
    /// preview, because a model told what is missing can say so instead of
    /// guessing, which is exactly what the system prompt asks of it.
    pub notes: Vec<String>,
}

/// The hosted assistant's answer, as the backend returns it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudAiAnswer {
    pub content: String,
    pub quota: CloudAiQuota,
}

/// How much of the included model's daily allowance this account has spent.
///
/// Counted and enforced on the backend, never here - a count the client
/// could edit would protect nothing, which is the reason the hosted model
/// proxies at all. This copy exists only so the UI can show it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudAiQuota {
    pub used: i32,
    pub limit: i32,
    /// Midnight UTC, when `used` returns to zero.
    pub resets_at: chrono::DateTime<chrono::Utc>,
}

/// What a completed non-streaming turn returns.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTurnResponse {
    pub content: String,
}
