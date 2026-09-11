use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{bool_input, text_input, validate_inputs, BlueprintHandler};

/// A self-hosted NATS server - a lightweight message broker for pub/sub,
/// request/reply and work queues between Applications.
///
/// It earns a blueprint for the same reason Redis does: it is a single
/// container with no configuration file worth editing, it is the sort of
/// thing somebody assembling their own infrastructure wants one click away,
/// and nothing in VibeSSH itself depends on it.
///
/// Two settings are command-line flags with no environment-variable
/// equivalent, so they have to be fields here rather than left to the
/// Environment tab - the same split `RedisBlueprint` documents for
/// `--requirepass`:
///
/// - **JetStream** (`-js --store_dir .`) turns on persistence. `.` is a
///   relative path, resolved against the container's working directory,
///   which `runtime::docker` bind-mounts to the Application's own directory -
///   the same trick `MariaDbBlueprint` and `RedisBlueprint` use, so a stream
///   survives a recreate rather than living in the container's writable
///   layer.
/// - **Auth token** (`--auth`) is the whole of NATS's simplest
///   authentication. Empty means anonymous, which is only safe while the
///   port stays private.
///
/// **Monitoring is on by default** (`-m 8222`), unlike anything else here,
/// and that is deliberate: without it NATS exposes no HTTP endpoint at all,
/// so the `HealthCheck` feature this blueprint advertises would have nothing
/// to probe beyond "the process exists". `/healthz` on 8222 is what makes an
/// HTTP health check mean something.
///
/// Verified against a real `nats:2` container before this was written, the
/// same bar `RedisBlueprint` set for itself: the container starts,
/// `/healthz` on 8222 answers `{"status":"ok"}`, JetStream creates its
/// `jetstream` directory inside the bind-mounted working directory rather
/// than in the container, and `--auth` is genuinely enforced - a client with
/// no token gets `Authorization Violation`, one with the token publishes.
/// Without the flag, anonymous clients are accepted, which is what the
/// field's help text warns about.
///
/// No `default_ports` - see `MariaDbBlueprint`'s own doc comment for why.
pub struct NatsBlueprint {
    definition: Blueprint,
}

impl NatsBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "nats".to_string(),
                name: "NATS".to_string(),
                description: "A self-hosted NATS server - a lightweight message broker for pub/sub, request/reply and work queues between Applications. Enable JetStream for persistence; data is stored in this Application's own working directory."
                    .to_string(),
                schema_version: 1,
                blueprint_version: 1,
                supported_runtime_types: vec![RuntimeType::Docker],
                features: vec![
                    BlueprintFeature::Logs,
                    BlueprintFeature::Environment,
                    BlueprintFeature::Ports,
                    BlueprintFeature::HealthCheck,
                    BlueprintFeature::Files,
                ],
                fields: vec![
                    BlueprintField {
                        key: "natsVersion".to_string(),
                        label: "NATS version".to_string(),
                        field_type: BlueprintFieldType::Text,
                        required: false,
                        default_value: Some(serde_json::Value::String("2".to_string())),
                        help_text: Some("A Docker Hub tag, e.g. 2, 2.10, or alpine.".to_string()),
                    },
                    BlueprintField {
                        key: "jetStream".to_string(),
                        label: "Enable JetStream (persistence)".to_string(),
                        field_type: BlueprintFieldType::Boolean,
                        required: false,
                        default_value: Some(serde_json::Value::Bool(true)),
                        help_text: Some(
                            "Stores streams in this Application's own working directory, so they survive a recreate. Off means messages exist only in memory."
                                .to_string(),
                        ),
                    },
                    BlueprintField {
                        key: "authToken".to_string(),
                        label: "Auth token".to_string(),
                        field_type: BlueprintFieldType::Text,
                        required: false,
                        default_value: None,
                        help_text: Some(
                            "Sets --auth. Leave empty to accept any client - only safe if this port stays private (see the Ports tab's visibility setting)."
                                .to_string(),
                        ),
                    },
                ],
                known_files: vec![],
                default_ports: vec![],
                connects_to: None,
                command_console: None,
                is_builtin: true,
            },
        }
    }
}

impl Default for NatsBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for NatsBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let version = text_input(inputs, &self.definition, "natsVersion")?;
        let jet_stream = bool_input(inputs, &self.definition, "jetStream")?;
        let token = text_input(inputs, &self.definition, "authToken")?;

        // `-m 8222` first, so the monitoring endpoint exists regardless of
        // what follows - see the type's doc comment for why it is not
        // optional.
        let mut command = vec!["-m".to_string(), "8222".to_string()];
        if jet_stream {
            command.push("-js".to_string());
            command.push("--store_dir".to_string());
            command.push(".".to_string());
        }
        if !token.trim().is_empty() {
            command.push("--auth".to_string());
            command.push(token);
        }

        let image = format!("nats:{}", version.trim());
        Ok(serde_json::json!({ "image": image, "command": command }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_jetstream_with_a_relative_store_dir_and_monitoring_on() {
        let blueprint = NatsBlueprint::new();
        let config = blueprint.render_runtime_config(&HashMap::new()).unwrap();
        assert_eq!(
            config,
            serde_json::json!({ "image": "nats:2", "command": ["-m", "8222", "-js", "--store_dir", "."] })
        );
    }

    /// The store directory has to stay relative. An absolute path would land
    /// in the container's writable layer, and a recreate - which the Ports
    /// and Environment tabs both trigger - would take every stream with it.
    #[test]
    fn the_store_dir_is_never_absolute() {
        let blueprint = NatsBlueprint::new();
        let config = blueprint.render_runtime_config(&HashMap::new()).unwrap();
        let command = config["command"].as_array().unwrap();
        let store_dir = command.iter().position(|arg| arg == "--store_dir").map(|i| command[i + 1].as_str().unwrap());
        assert_eq!(store_dir, Some("."));
    }

    #[test]
    fn turning_jetstream_off_drops_the_store_flags_but_keeps_monitoring() {
        let blueprint = NatsBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("jetStream".to_string(), serde_json::json!(false));
        let config = blueprint.render_runtime_config(&inputs).unwrap();
        assert_eq!(config["command"], serde_json::json!(["-m", "8222"]));
    }

    #[test]
    fn an_auth_token_is_passed_through_as_its_own_argument() {
        let blueprint = NatsBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("authToken".to_string(), serde_json::json!("s3cret"));
        let config = blueprint.render_runtime_config(&inputs).unwrap();
        assert_eq!(config["command"], serde_json::json!(["-m", "8222", "-js", "--store_dir", ".", "--auth", "s3cret"]));
    }

    /// A blank token means anonymous, not `--auth ""` - which NATS would
    /// treat as a token that is the empty string, refusing every client that
    /// did not send one.
    #[test]
    fn a_blank_token_is_left_off_entirely() {
        let blueprint = NatsBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("authToken".to_string(), serde_json::json!("   "));
        let config = blueprint.render_runtime_config(&inputs).unwrap();
        assert!(!config["command"].as_array().unwrap().iter().any(|arg| arg == "--auth"));
    }

    #[test]
    fn a_custom_version_overrides_the_default_image_tag() {
        let blueprint = NatsBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("natsVersion".to_string(), serde_json::json!("2.10-alpine"));
        let config = blueprint.render_runtime_config(&inputs).unwrap();
        assert_eq!(config["image"], serde_json::json!("nats:2.10-alpine"));
    }
}
