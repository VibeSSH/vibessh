//! What the Vibe AI assistant does, as opposed to how any one piece of it
//! works.
//!
//! This is the only place that knows the order: read the configuration,
//! refuse early if the assistant is off, collect context if the mode calls
//! for it, look up documentation, assemble the prompt, call the provider.
//! Every step it calls is in `ai::`, and none of them know about each
//! other.
//!
//! **Conversations are not persisted here.** The message history arrives
//! from the frontend on every turn and is thrown away when the turn ends.
//! That is a deliberate choice for this version, not an omission: a
//! conversation about a broken Node is a transcript of that Node's
//! configuration and logs, and writing it to disk would create a second,
//! longer-lived copy of exactly the material the rest of this feature works
//! to keep contained. If persistence is added later it needs its own
//! decision about retention and about what a "clear conversation" button
//! actually deletes.

use std::path::Path;
use std::sync::Arc;

use crate::ai::context::AiContextBuilder;
use crate::ai::knowledge::KnowledgeSource;
use crate::ai::openai_compatible::OpenAiCompatibleProvider;
use crate::ai::prompt;
use crate::ai::provider::{AiProvider, AiRequest};
use crate::errors::{AppError, AppResult};
use crate::models::{
    AiConfig, AiConfigView, AiContextBundle, AiContextRef, AiMode, AiProviderKind, AiRole, AiTurnRequest, SetAiConfigInput,
};
use crate::runtime::local_process::LocalProcessManager;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;
use crate::storage::{ai_config, credentials};

/// A ceiling on the answer. The assistant is asked for three short points;
/// this is roughly four times what that needs, which leaves room for a
/// code block without leaving room for a runaway generation on a metered
/// account.
const MAX_ANSWER_TOKENS: u32 = 1200;

/// Low, because the job is to read a supplied snapshot and say what is
/// missing rather than to write anything. Not zero: some endpoints treat
/// exactly 0 as "use the default", which would silently undo this.
const TEMPERATURE: f32 = 0.2;

/// How many documentation passages to attach.
const KNOWLEDGE_SNIPPETS: usize = 3;

/// The current settings, plus whether a key is stored.
pub fn ai_config_view(config_dir: &Path) -> AppResult<AiConfigView> {
    let config = ai_config::load_ai_config(config_dir)?;
    let has_api_key = credentials::load_ai_api_key()?.is_some_and(|key| !key.is_empty());
    Ok(AiConfigView { config, has_api_key })
}

/// Saves the settings, and the key if one was supplied.
///
/// A blank `api_key` leaves the stored one alone - the frontend never holds
/// the real key, so a blank field means "unchanged", not "clear it". The
/// key is cleared explicitly instead: turning the assistant off removes it,
/// so a user who decides against this feature is not left with a live
/// credential in their keyring for a service they no longer use.
pub fn set_ai_config(config_dir: &Path, input: SetAiConfigInput) -> AppResult<AiConfigView> {
    let config = AiConfig {
        enabled: input.enabled,
        provider: input.provider,
        base_url: input.base_url.trim().to_string(),
        model: input.model.trim().to_string(),
    };

    if input.enabled {
        let key = input.api_key.trim();
        if !key.is_empty() {
            credentials::store_ai_api_key(key)?;
        }
    } else {
        // Not `forget_secret`'s log-and-continue treatment: this one is
        // triggered by an explicit user action, so a failure to carry it out
        // has to be visible rather than logged.
        credentials::delete_ai_api_key()?;
    }

    ai_config::save_ai_config(config_dir, &config)?;
    ai_config_view(config_dir)
}

/// Builds the configured provider, or explains why it cannot.
///
/// Every refusal here is `AiNotConfigured`, which the UI turns into a
/// sentence pointing at Settings. Distinguishing "no model" from "no
/// endpoint" in the error code would be precision the user cannot act on
/// differently - both send them to the same form.
pub fn resolve_provider(config_dir: &Path) -> AppResult<(AiConfig, Box<dyn AiProvider>)> {
    let config = ai_config::load_ai_config(config_dir)?;
    if !config.enabled || config.base_url.is_empty() || config.model.is_empty() {
        return Err(AppError::AiNotConfigured);
    }
    // Absent is fine - a self-hosted endpoint needs no key. See
    // `OpenAiCompatibleProvider::request_builder`.
    let api_key = credentials::load_ai_api_key()?.unwrap_or_default();
    let provider: Box<dyn AiProvider> = match config.provider {
        AiProviderKind::OpenAiCompatible => Box::new(OpenAiCompatibleProvider::new(&config.base_url, &api_key)?),
    };
    Ok((config, provider))
}

/// One real round trip, so Test connection proves the endpoint, the key and
/// the model name together.
///
/// Deliberately not a `GET /models` probe, which several OpenAI-compatible
/// endpoints do not implement and which would pass while the configured
/// model does not exist - the failure users actually hit.
pub async fn test_ai_connection(config_dir: &Path) -> AppResult<()> {
    let (config, provider) = resolve_provider(config_dir)?;
    let request = AiRequest {
        model: config.model,
        messages: vec![crate::ai::provider::ChatMessage {
            role: crate::ai::provider::ChatRole::User,
            content: "Reply with the single word: ok".to_string(),
        }],
        max_tokens: Some(16),
        temperature: Some(0.0),
    };
    provider.complete(&request).await?;
    Ok(())
}

/// Collects the snapshot for a turn, or returns `None` when there is
/// nothing to collect.
///
/// `Ask` never collects, whatever the frontend passed - the mode is the
/// authority on that, not the caller, so an `Ask` turn cannot be talked
/// into reading a Node by a request that happens to carry a context id.
#[allow(clippy::too_many_arguments)]
pub async fn build_ai_context(
    applications: &ApplicationRepository,
    servers: &ServerRepository,
    networks: &NodeNetworkRepository,
    firewall_rules: &FirewallRuleRepository,
    ssh_sessions: &SshSessionManager,
    local_processes: &Arc<LocalProcessManager>,
    log_capture: &LogCaptureStore,
    mode: AiMode,
    reference: Option<AiContextRef>,
) -> Option<AiContextBundle> {
    if mode != AiMode::Diagnose {
        return None;
    }
    let reference = reference?;
    let builder = AiContextBuilder { applications, servers, networks, firewall_rules, ssh_sessions, local_processes, log_capture };
    Some(builder.build(reference).await)
}

/// Assembles and runs one turn.
///
/// Takes the provider and the knowledge source as trait objects rather than
/// building them itself, which is what lets the whole orchestration - mode
/// handling, context attachment, prompt assembly, error propagation - be
/// tested against a mock with no network and no configuration.
pub async fn run_turn(
    provider: &dyn AiProvider,
    knowledge: &dyn KnowledgeSource,
    model: &str,
    context: Option<&AiContextBundle>,
    request: &AiTurnRequest,
    on_delta: &(dyn Fn(String) + Send + Sync),
) -> AppResult<String> {
    let last_user_message = request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == AiRole::User)
        .map(|message| message.content.as_str())
        .unwrap_or_default();
    if last_user_message.trim().is_empty() {
        return Err(AppError::InvalidInput("there is no question to send".to_string()));
    }

    let snippets: Vec<String> =
        knowledge.search(last_user_message, KNOWLEDGE_SNIPPETS).iter().map(|snippet| snippet.to_prompt_text()).collect();

    let messages = prompt::build_messages(context, &snippets, &request.messages);
    let ai_request =
        AiRequest { model: model.to_string(), messages, max_tokens: Some(MAX_ANSWER_TOKENS), temperature: Some(TEMPERATURE) };

    provider.stream(&ai_request, on_delta).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::knowledge::{KnowledgeSnippet, KnowledgeSource};
    use crate::ai::provider::{AiRequest, ChatRole};
    use crate::models::{AiMessage, AiRole};
    use std::sync::Mutex;

    /// Records what it was asked and answers whatever it was told to.
    /// Nothing in these tests touches the network, which the brief for this
    /// feature required explicitly.
    struct MockProvider {
        answer: AppResult<String>,
        seen: Mutex<Option<AiRequest>>,
    }

    impl MockProvider {
        fn answering(text: &str) -> Self {
            Self { answer: Ok(text.to_string()), seen: Mutex::new(None) }
        }

        fn failing(error: AppError) -> Self {
            Self { answer: Err(error), seen: Mutex::new(None) }
        }

        fn last_request(&self) -> AiRequest {
            self.seen.lock().unwrap().clone().expect("the provider was never called")
        }
    }

    #[async_trait::async_trait]
    impl AiProvider for MockProvider {
        async fn complete(&self, request: &AiRequest) -> AppResult<String> {
            *self.seen.lock().unwrap() = Some(request.clone());
            match &self.answer {
                Ok(text) => Ok(text.clone()),
                // `AppError` is not `Clone` - it carries `thiserror`
                // sources - so the variant under test is rebuilt rather
                // than copied.
                Err(AppError::AiAuthFailed) => Err(AppError::AiAuthFailed),
                Err(AppError::AiRateLimited) => Err(AppError::AiRateLimited),
                Err(AppError::AiProviderUnavailable) => Err(AppError::AiProviderUnavailable),
                Err(AppError::AiModelUnavailable { model }) => Err(AppError::AiModelUnavailable { model: model.clone() }),
                Err(AppError::Timeout { operation, seconds }) => Err(AppError::Timeout { operation, seconds: *seconds }),
                Err(other) => Err(AppError::Internal(other.to_string())),
            }
        }
    }

    struct NoKnowledge;

    impl KnowledgeSource for NoKnowledge {
        fn search(&self, _query: &str, _limit: usize) -> Vec<KnowledgeSnippet> {
            Vec::new()
        }
    }

    struct OneSnippet;

    impl KnowledgeSource for OneSnippet {
        fn search(&self, _query: &str, _limit: usize) -> Vec<KnowledgeSnippet> {
            vec![KnowledgeSnippet {
                title: "Ports".to_string(),
                source: "Applications architecture".to_string(),
                body: "A published port is reachable from outside the Node.".to_string(),
            }]
        }
    }

    fn ask(text: &str) -> AiTurnRequest {
        AiTurnRequest { mode: AiMode::Ask, context: None, messages: vec![AiMessage { role: AiRole::User, content: text.to_string() }] }
    }

    fn no_deltas() -> impl Fn(String) + Send + Sync {
        |_| {}
    }

    #[tokio::test]
    async fn a_plain_question_reaches_the_provider_with_the_system_prompt_in_front() {
        let provider = MockProvider::answering("the port is taken");
        let answer = run_turn(&provider, &NoKnowledge, "gpt-4o-mini", None, &ask("why is it down?"), &no_deltas()).await.unwrap();
        assert_eq!(answer, "the port is taken");

        let sent = provider.last_request();
        assert_eq!(sent.model, "gpt-4o-mini");
        assert_eq!(sent.messages[0].role, ChatRole::System);
        assert_eq!(sent.messages[0].content, prompt::SYSTEM_PROMPT);
        assert_eq!(sent.messages.last().unwrap().content, "why is it down?");
    }

    #[tokio::test]
    async fn documentation_snippets_are_attached_when_the_knowledge_source_finds_any() {
        let provider = MockProvider::answering("ok");
        run_turn(&provider, &OneSnippet, "m", None, &ask("how do ports work?"), &no_deltas()).await.unwrap();
        let joined: String = provider.last_request().messages.iter().map(|m| m.content.clone()).collect();
        assert!(joined.contains("A published port is reachable"));
        assert!(joined.contains("DOCUMENTATION"));
    }

    #[tokio::test]
    async fn collected_context_and_its_gaps_both_reach_the_model() {
        let provider = MockProvider::answering("ok");
        let bundle = AiContextBundle {
            summary: "Application\nName: paper-survival\nStored status: failed".to_string(),
            sources: vec!["Application".to_string()],
            notes: vec!["the live status could not be checked".to_string()],
        };
        run_turn(&provider, &NoKnowledge, "m", Some(&bundle), &ask("what is wrong?"), &no_deltas()).await.unwrap();
        let joined: String = provider.last_request().messages.iter().map(|m| m.content.clone()).collect();
        assert!(joined.contains("paper-survival"));
        // The gap has to travel with the snapshot; it is what the system
        // prompt's "say what is missing" instruction acts on.
        assert!(joined.contains("could not be checked"));
    }

    /// The default `AiProvider::stream` completes and emits once, so a
    /// provider with no streaming support still drives the UI's incremental
    /// rendering rather than failing.
    #[tokio::test]
    async fn a_non_streaming_provider_still_produces_a_delta() {
        let provider = MockProvider::answering("hello");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let seen = Arc::clone(&seen);
            move |delta: String| seen.lock().unwrap().push(delta)
        };
        let answer = run_turn(&provider, &NoKnowledge, "m", None, &ask("hi"), &sink).await.unwrap();
        assert_eq!(answer, "hello");
        assert_eq!(*seen.lock().unwrap(), vec!["hello".to_string()]);
    }

    #[tokio::test]
    async fn an_empty_question_is_refused_before_anything_is_sent() {
        let provider = MockProvider::answering("should never be reached");
        let request = AiTurnRequest { mode: AiMode::Ask, context: None, messages: vec![AiMessage { role: AiRole::User, content: "   ".into() }] };
        let error = run_turn(&provider, &NoKnowledge, "m", None, &request, &no_deltas()).await.unwrap_err();
        assert!(matches!(error, AppError::InvalidInput(_)));
        assert!(provider.seen.lock().unwrap().is_none(), "nothing should have been sent");
    }

    #[tokio::test]
    async fn a_rejected_key_surfaces_as_the_auth_code_and_not_as_prose() {
        let provider = MockProvider::failing(AppError::AiAuthFailed);
        let error = run_turn(&provider, &NoKnowledge, "m", None, &ask("hi"), &no_deltas()).await.unwrap_err();
        assert_eq!(error.code(), crate::errors::ErrorCode::AiAuthFailed);
    }

    #[tokio::test]
    async fn a_provider_timeout_keeps_its_own_code() {
        let provider = MockProvider::failing(AppError::Timeout { operation: "the AI request", seconds: 90 });
        let error = run_turn(&provider, &NoKnowledge, "m", None, &ask("hi"), &no_deltas()).await.unwrap_err();
        assert_eq!(error.code(), crate::errors::ErrorCode::Timeout);
    }

    #[tokio::test]
    async fn a_rate_limit_and_an_unreachable_provider_stay_distinguishable() {
        let limited = MockProvider::failing(AppError::AiRateLimited);
        assert_eq!(
            run_turn(&limited, &NoKnowledge, "m", None, &ask("hi"), &no_deltas()).await.unwrap_err().code(),
            crate::errors::ErrorCode::AiRateLimited
        );
        let down = MockProvider::failing(AppError::AiProviderUnavailable);
        assert_eq!(
            run_turn(&down, &NoKnowledge, "m", None, &ask("hi"), &no_deltas()).await.unwrap_err().code(),
            crate::errors::ErrorCode::AiProviderUnavailable
        );
    }
}
