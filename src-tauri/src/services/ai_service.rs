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
use crate::ai::hosted::HostedProvider;
use crate::ai::knowledge::KnowledgeSource;
use crate::ai::openai_compatible::OpenAiCompatibleProvider;
use crate::ai::prompt;
use crate::ai::provider::{AiProvider, AiRequest};
use crate::ai::skills;
use crate::errors::{AppError, AppResult};
use crate::models::{
    AiConfig, AiConfigView, AiContextBundle, AiContextRef, AiMode, AiProviderKind, AiRole, AiTurnRequest, CloudAiQuota,
    SetAiConfigInput,
};
use crate::state::CloudState;
use crate::runtime::local_process::LocalProcessManager;
use crate::state::SshSessionManager;
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::firewall_rule_repository::FirewallRuleRepository;
use crate::storage::log_capture::LogCaptureStore;
use crate::storage::node_network_repository::NodeNetworkRepository;
use crate::storage::server_repository::ServerRepository;
use crate::services::cloud_service::cloud_ai_endpoint;
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

/// How many diagnostic playbooks to attach.
///
/// Two, not five. The failure this feature had on its first real use was an
/// answer that surveyed every possible cause instead of naming the one the
/// log identified; handing the model five playbooks would invite exactly
/// that, in a more authoritative voice. Two leaves room for a genuine
/// ambiguity - a crash that is either memory or a plugin - without inviting
/// a list.
const PLAYBOOKS: usize = 2;

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

    if input.enabled && input.provider == AiProviderKind::OpenAiCompatible {
        let key = input.api_key.trim();
        if !key.is_empty() {
            credentials::store_ai_api_key(key)?;
        }
    } else {
        // Switching to the included model clears the personal key too.
        // Keeping a live credential in the keyring for a provider the
        // user has stopped using is the same leftover `forget_secret`
        // exists to prevent elsewhere.
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
pub async fn resolve_provider(config_dir: &Path, cloud: &CloudState) -> AppResult<(AiConfig, Box<dyn AiProvider>)> {
    let config = ai_config::load_ai_config(config_dir)?;
    if !config.enabled {
        return Err(AppError::AiNotConfigured);
    }
    match config.provider {
        AiProviderKind::OpenAiCompatible => {
            if config.base_url.is_empty() || config.model.is_empty() {
                return Err(AppError::AiNotConfigured);
            }
            // Absent is fine - a self-hosted endpoint needs no key. See
            // `OpenAiCompatibleProvider::request_builder`.
            let api_key = credentials::load_ai_api_key()?.unwrap_or_default();
            let provider: Box<dyn AiProvider> = Box::new(OpenAiCompatibleProvider::new(&config.base_url, &api_key)?);
            Ok((config, provider))
        }
        AiProviderKind::VibeSshHosted => {
            // Being signed out is `Unauthorized`, not `AiNotConfigured`:
            // the assistant is configured correctly and the remedy is a
            // login, which the frontend already routes that code to.
            let (backend_url, token) = cloud_ai_endpoint(cloud).await?;
            let provider: Box<dyn AiProvider> = Box::new(HostedProvider::new(&backend_url, &token));
            Ok((config, provider))
        }
    }
}

/// The account's remaining allowance for the included model.
///
/// `None` when the user is not on the hosted provider - there is no
/// allowance to report for a personal API key, and showing a quota that
/// does not apply would be worse than showing none.
pub async fn ai_quota(config_dir: &Path, cloud: &CloudState) -> AppResult<Option<CloudAiQuota>> {
    let config = ai_config::load_ai_config(config_dir)?;
    if config.provider != AiProviderKind::VibeSshHosted {
        return Ok(None);
    }
    let (backend_url, token) = cloud_ai_endpoint(cloud).await?;
    HostedProvider::new(&backend_url, &token).quota().await.map(Some)
}

/// One real round trip, so Test connection proves the endpoint, the key and
/// the model name together.
///
/// Deliberately not a `GET /models` probe, which several OpenAI-compatible
/// endpoints do not implement and which would pass while the configured
/// model does not exist - the failure users actually hit.
pub async fn test_ai_connection(config_dir: &Path, cloud: &CloudState) -> AppResult<()> {
    let (config, provider) = resolve_provider(config_dir, cloud).await?;
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

    // Timed because "the assistant feels slow" is otherwise unanswerable:
    // a turn waits on this and then on a model, and the two are
    // indistinguishable from outside. The panel already names which phase
    // it is in; this puts a number on the first one.
    let started = std::time::Instant::now();
    let bundle = builder.build(reference).await;
    log::info!("collected the AI context in {} ms", started.elapsed().as_millis());
    Some(bundle)
}

/// Pulls the blueprint id back out of the collected summary.
///
/// The context is built as text on purpose - it is what the model reads and
/// what the user previews - so this reads the one line it needs rather than
/// threading a second, structured copy of the same fact through every layer
/// that would then have to be kept in step with it.
fn blueprint_from_summary(summary: &str) -> Option<String> {
    summary
        .lines()
        .find_map(|line| line.strip_prefix("Blueprint: "))
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_string)
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

    // Matched against the collected snapshot, which is where the logs are -
    // a playbook fires on a signature in the evidence, not on the topic. The
    // blueprint is read out of the summary rather than passed separately so
    // that `Ask` turns, which have no context at all, cannot accidentally
    // pull in blueprint-scoped advice about an Application nobody mentioned.
    let summary = context.map(|bundle| bundle.summary.as_str());
    let blueprint = summary.and_then(blueprint_from_summary);
    let playbooks = skills::match_skills(summary, last_user_message, blueprint.as_deref(), PLAYBOOKS);

    let messages = prompt::build_messages(request.mode, context, &playbooks, &snippets, &request.messages);
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

    /// The section header, not the bare word: the system prompt itself
    /// explains what to do when a PLAYBOOKS section is present, so searching
    /// the whole request for the word matches even when nothing was
    /// attached. That false pass is exactly what this caught.
    const PLAYBOOK_HEADING: &str = "PLAYBOOKS (a known cause";

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
        assert_eq!(sent.messages[0].content, prompt::system_prompt(AiMode::Ask));
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

    /// The regression this whole `skills` module exists for. The first real
    /// Diagnose turn produced a long answer surveying every cause of a
    /// Minecraft server that will not start, while the log in front of it
    /// said `level.dat`. The playbook has to reach the request, and it has to
    /// be selected from the log rather than from the question.
    #[tokio::test]
    async fn a_corrupt_world_in_the_logs_attaches_its_playbook() {
        let provider = MockProvider::answering("ok");
        let bundle = AiContextBundle {
            summary: "Application
Name: survival
Blueprint: paper (version 1)

Recent log lines
[12:00:01] [Server thread/ERROR]: Failed to load level.dat"
                .to_string(),
            sources: vec!["Application".to_string(), "Logs".to_string()],
            notes: vec![],
        };
        run_turn(&provider, &NoKnowledge, "m", Some(&bundle), &ask("nie dziala"), &no_deltas()).await.unwrap();

        let joined: String = provider.last_request().messages.iter().map(|m| m.content.clone()).collect();
        assert!(joined.contains(PLAYBOOK_HEADING));
        assert!(joined.contains("level.dat_old"), "the world-corruption playbook should have been attached");
    }

    /// The mirror image, and the more important half: an Application that is
    /// simply running must not drag in a playbook. Attaching one would push
    /// the model toward diagnosing a problem that is not there.
    #[tokio::test]
    async fn a_healthy_application_attaches_no_playbook() {
        let provider = MockProvider::answering("ok");
        let bundle = AiContextBundle {
            summary: "Application
Name: survival
Blueprint: paper (version 1)
Stored status: running".to_string(),
            sources: vec!["Application".to_string()],
            notes: vec![],
        };
        run_turn(&provider, &NoKnowledge, "m", Some(&bundle), &ask("how do I add a plugin?"), &no_deltas()).await.unwrap();

        let joined: String = provider.last_request().messages.iter().map(|m| m.content.clone()).collect();
        assert!(!joined.contains(PLAYBOOK_HEADING), "a running Application should not have had a playbook attached");
    }

    #[test]
    fn the_blueprint_is_read_back_out_of_the_summary() {
        assert_eq!(blueprint_from_summary("Name: x
Blueprint: paper (version 1)
").as_deref(), Some("paper"));
        assert_eq!(blueprint_from_summary("Name: x
Blueprint: nodejs-bot (version 2)").as_deref(), Some("nodejs-bot"));
        assert_eq!(blueprint_from_summary("Node
Name: vps"), None);
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
