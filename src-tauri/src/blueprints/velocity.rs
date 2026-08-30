use std::collections::HashMap;

use crate::errors::{AppError, AppResult};
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, KnownFile, RuntimeType};
use crate::services::latest_velocity_build;

use super::{text_input, text_list_input, validate_inputs, BlueprintHandler, ProvisionContext};

/// Not a user-facing wizard field - see `paper::JAR_FILENAME_KEY`'s own
/// doc comment for the full reasoning (identical here).
const JAR_FILENAME_KEY: &str = "__jarFilename";

/// A Minecraft proxy - the jar is downloaded automatically from PaperMC for
/// the chosen Velocity version, same idea as `PaperBlueprint` but for the
/// proxy instead of the game server itself. No EULA: Velocity is Paper-
/// licensed open source, not a Mojang-licensed game server, so there's
/// nothing to accept before it can run.
pub struct VelocityBlueprint {
    definition: Blueprint,
}

impl VelocityBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "velocity".to_string(),
                name: "Velocity".to_string(),
                description: "A Minecraft proxy - the jar is downloaded and kept up to date automatically.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                supported_runtime_types: vec![RuntimeType::LocalProcess, RuntimeType::RemoteProcess, RuntimeType::Systemd],
                features: vec![BlueprintFeature::Console, BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck, BlueprintFeature::Databases, BlueprintFeature::Files],
                fields: vec![
                    BlueprintField {
                        key: "velocityVersion".to_string(),
                        label: "Velocity version".to_string(),
                        field_type: BlueprintFieldType::PapermcVersion,
                        required: true,
                        default_value: None,
                        help_text: Some("The matching Velocity jar is downloaded automatically.".to_string()),
                    },
                    BlueprintField {
                        key: "javaBinary".to_string(),
                        label: "Java version".to_string(),
                        field_type: BlueprintFieldType::JavaVersion,
                        required: false,
                        default_value: Some(serde_json::Value::String("java".to_string())),
                        help_text: Some("Detected Java installations - pick one, or enter a path yourself.".to_string()),
                    },
                    BlueprintField {
                        key: "jvmArgs".to_string(),
                        label: "JVM arguments".to_string(),
                        field_type: BlueprintFieldType::TextList,
                        required: false,
                        default_value: Some(serde_json::json!([])),
                        help_text: Some("Flags passed to the JVM itself, before -jar - e.g. -Xmx1G.".to_string()),
                    },
                    BlueprintField {
                        key: "programArgs".to_string(),
                        label: "Program arguments".to_string(),
                        field_type: BlueprintFieldType::TextList,
                        required: false,
                        default_value: Some(serde_json::json!([])),
                        help_text: Some("Arguments passed to the proxy jar itself.".to_string()),
                    },
                ],
                known_files: vec![
                    KnownFile { path: "velocity.toml".to_string(), label: "velocity.toml".to_string() },
                    KnownFile { path: "forwarding.secret".to_string(), label: "forwarding.secret".to_string() },
                ],
                is_builtin: true,
            },
        }
    }
}

impl Default for VelocityBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for VelocityBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;

        let jar_filename = inputs
            .get(JAR_FILENAME_KEY)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AppError::Internal("Velocity's jar filename wasn't set by provision() before rendering".into()))?;

        let java_binary = text_input(inputs, &self.definition, "javaBinary")?;
        let jvm_args = text_list_input(inputs, &self.definition, "jvmArgs")?;
        let program_args = text_list_input(inputs, &self.definition, "programArgs")?;

        let mut args = jvm_args;
        args.push("-jar".to_string());
        args.push(jar_filename.to_string());
        args.extend(program_args);

        Ok(serde_json::json!({ "command": java_binary, "args": args }))
    }

    async fn provision(
        &self,
        inputs: &HashMap<String, serde_json::Value>,
        context: &ProvisionContext<'_>,
    ) -> AppResult<HashMap<String, serde_json::Value>> {
        validate_inputs(&self.definition, inputs)?;
        let version = text_input(inputs, &self.definition, "velocityVersion")?;
        if version.trim().is_empty() {
            return Err(AppError::InvalidInput("a Velocity version is required".into()));
        }

        let build = latest_velocity_build(&version).await?;
        download_file(context, &build.url, &build.filename).await?;

        let mut discovered = HashMap::new();
        discovered.insert(JAR_FILENAME_KEY.to_string(), serde_json::Value::String(build.filename));
        Ok(discovered)
    }
}

async fn download_file(context: &ProvisionContext<'_>, url: &str, filename: &str) -> AppResult<()> {
    match &context.connection {
        None => {
            let bytes = reqwest::get(url)
                .await
                .map_err(|err| AppError::Connection(format!("couldn't download {filename}: {err}")))?
                .bytes()
                .await
                .map_err(|err| AppError::Connection(format!("couldn't download {filename}: {err}")))?;
            let path = std::path::Path::new(context.working_directory).join(filename);
            tokio::fs::write(&path, &bytes).await.map_err(|err| AppError::InvalidInput(format!("couldn't save {filename}: {err}")))
        }
        // Same reasoning as PaperBlueprint's own download_file: the remote
        // host pulls the jar directly from papermc.io itself via curl,
        // rather than routing it through the user's own desktop twice.
        Some(connection) => {
            let path = format!("{}/{}", context.working_directory.trim_end_matches('/'), filename);
            let output = connection.execute_command(&format!("curl -fsSL -o {} {}", shell_quote(&path), shell_quote(url))).await?;
            if output.exit_code != 0 {
                let detail = output.stderr.trim();
                let detail = if detail.is_empty() { "curl failed".to_string() } else { detail.to_string() };
                return Err(AppError::Connection(format!("couldn't download {filename} on the remote host: {detail}")));
            }
            Ok(())
        }
    }
}

/// POSIX single-quote shell escaping - duplicated across
/// `runtime::remote_process`, `runtime::docker`,
/// `services::application_service`, and `blueprints::paper`; see
/// `runtime::remote_process`'s copy for the full reasoning.
fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs_with_jar(jar_filename: Option<&str>) -> HashMap<String, serde_json::Value> {
        let mut inputs = HashMap::new();
        inputs.insert("velocityVersion".to_string(), serde_json::json!("3.4.0"));
        if let Some(filename) = jar_filename {
            inputs.insert(JAR_FILENAME_KEY.to_string(), serde_json::json!(filename));
        }
        inputs
    }

    #[test]
    fn render_runtime_config_rejects_a_missing_jar_filename() {
        let blueprint = VelocityBlueprint::new();
        let inputs = inputs_with_jar(None);
        assert!(blueprint.render_runtime_config(&inputs).is_err());
    }

    #[test]
    fn render_runtime_config_builds_command_and_args_with_the_downloaded_jar() {
        let blueprint = VelocityBlueprint::new();
        let inputs = inputs_with_jar(Some("velocity-3.4.0-566.jar"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "command": "java", "args": ["-jar", "velocity-3.4.0-566.jar"] }));
    }

    #[test]
    fn render_runtime_config_places_jvm_args_before_jar_and_program_args_after() {
        let blueprint = VelocityBlueprint::new();
        let mut inputs = inputs_with_jar(Some("velocity-3.4.0-566.jar"));
        inputs.insert("jvmArgs".to_string(), serde_json::json!(["-Xmx1G"]));
        inputs.insert("programArgs".to_string(), serde_json::json!(["--example"]));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "command": "java", "args": ["-Xmx1G", "-jar", "velocity-3.4.0-566.jar", "--example"] }));
    }

    #[tokio::test]
    async fn provision_downloads_a_real_jar_locally() {
        let blueprint = VelocityBlueprint::new();
        let inputs = inputs_with_jar(None);
        let working_directory = std::env::temp_dir().join(format!("vibessh-velocity-provision-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&working_directory).await.unwrap();
        let context = ProvisionContext { working_directory: working_directory.to_str().unwrap(), connection: None };

        let result = blueprint.provision(&inputs, &context).await;
        let Ok(discovered) = result else {
            eprintln!("skipping: papermc.io unreachable from this environment ({:?})", result.err());
            tokio::fs::remove_dir_all(&working_directory).await.ok();
            return;
        };

        let jar_filename = discovered[JAR_FILENAME_KEY].as_str().unwrap().to_string();
        assert!(working_directory.join(&jar_filename).is_file());

        tokio::fs::remove_dir_all(&working_directory).await.ok();
    }
}
