use std::collections::HashMap;

use crate::errors::{AppError, AppResult};
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};
use crate::services::latest_paper_build;

use super::{bool_input, text_input, text_list_input, validate_inputs, BlueprintHandler, ProvisionContext};

/// Not a user-facing wizard field - `provision()` writes the real,
/// downloaded jar's filename here (only known once the download actually
/// happens; PaperMC's build numbers change over time, so it can't be
/// hardcoded or guessed), and `render_runtime_config` reads it back. Kept
/// out of `Blueprint::fields` entirely so the wizard never shows the user
/// an empty "jar filename" box they have no reason to fill in themselves -
/// that's the whole point of this blueprint over `generic-java`.
const JAR_FILENAME_KEY: &str = "__jarFilename";

/// A high-performance Minecraft server - the server jar is downloaded
/// automatically from PaperMC for the chosen Minecraft version, and the
/// Minecraft EULA is written out once the user has explicitly accepted it,
/// rather than the user having to source and place `paper.jar` themselves
/// the way `generic-java` requires.
pub struct PaperBlueprint {
    definition: Blueprint,
}

impl PaperBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "paper".to_string(),
                name: "Paper".to_string(),
                description: "A high-performance Minecraft server - the server jar is downloaded and kept up to date automatically.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                supported_runtime_types: vec![RuntimeType::LocalProcess, RuntimeType::RemoteProcess, RuntimeType::Systemd],
                features: vec![BlueprintFeature::Console, BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports],
                fields: vec![
                    BlueprintField {
                        key: "minecraftVersion".to_string(),
                        label: "Minecraft version".to_string(),
                        field_type: BlueprintFieldType::PapermcVersion,
                        required: true,
                        default_value: None,
                        help_text: Some("The matching Paper server jar is downloaded automatically.".to_string()),
                    },
                    BlueprintField {
                        key: "eulaAccepted".to_string(),
                        label: "I accept the Minecraft EULA (https://www.minecraft.net/eula)".to_string(),
                        field_type: BlueprintFieldType::Boolean,
                        required: true,
                        default_value: Some(serde_json::Value::Bool(false)),
                        help_text: None,
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
                        help_text: Some("Flags passed to the JVM itself, before -jar - e.g. -Xmx2G.".to_string()),
                    },
                    BlueprintField {
                        key: "programArgs".to_string(),
                        label: "Program arguments".to_string(),
                        field_type: BlueprintFieldType::TextList,
                        required: false,
                        default_value: Some(serde_json::json!(["nogui"])),
                        help_text: Some("Arguments passed to the server jar itself.".to_string()),
                    },
                ],
                is_builtin: true,
            },
        }
    }
}

impl Default for PaperBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

/// Checked in both `provision` (before spending a download on an
/// application that can't legally run yet) and `render_runtime_config`
/// (in case a future caller ever renders without provisioning first) -
/// `validate_inputs`'s generic "is this field present" check accepts a
/// `false` boolean as "present", which isn't the same as "accepted".
fn require_eula_accepted(definition: &Blueprint, inputs: &HashMap<String, serde_json::Value>) -> AppResult<()> {
    if !bool_input(inputs, definition, "eulaAccepted")? {
        return Err(AppError::InvalidInput("the Minecraft EULA must be accepted to create a Paper application".into()));
    }
    Ok(())
}

#[async_trait::async_trait]
impl BlueprintHandler for PaperBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        require_eula_accepted(&self.definition, inputs)?;

        let jar_filename = inputs
            .get(JAR_FILENAME_KEY)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AppError::Internal("Paper's jar filename wasn't set by provision() before rendering".into()))?;

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
        require_eula_accepted(&self.definition, inputs)?;
        let version = text_input(inputs, &self.definition, "minecraftVersion")?;
        if version.trim().is_empty() {
            return Err(AppError::InvalidInput("a Minecraft version is required".into()));
        }

        let build = latest_paper_build(&version).await?;
        download_file(context, &build.url, &build.filename).await?;
        write_eula(context).await?;

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
        // The remote host downloads directly from papermc.io itself (via
        // curl) rather than VibeSSH pulling the jar through the user's own
        // desktop connection and re-uploading it over SFTP - meaningfully
        // faster for a ~50MB server jar, and the only sane choice on a
        // metered or slow desktop connection.
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

async fn write_eula(context: &ProvisionContext<'_>) -> AppResult<()> {
    const EULA_CONTENT: &[u8] = b"eula=true\n";
    match &context.connection {
        None => {
            let path = std::path::Path::new(context.working_directory).join("eula.txt");
            tokio::fs::write(&path, EULA_CONTENT).await.map_err(|err| AppError::InvalidInput(format!("couldn't write eula.txt: {err}")))
        }
        Some(connection) => {
            let path = format!("{}/eula.txt", context.working_directory.trim_end_matches('/'));
            connection.write_file(&path, EULA_CONTENT).await
        }
    }
}

/// POSIX single-quote shell escaping - see `runtime::remote_process`'s copy
/// of the same function for the full reasoning; duplicated rather than
/// shared, same as it already is across `runtime::remote_process`,
/// `runtime::docker`, and `services::application_service`.
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

    fn accepted_inputs(jar_filename: Option<&str>) -> HashMap<String, serde_json::Value> {
        let mut inputs = HashMap::new();
        inputs.insert("minecraftVersion".to_string(), serde_json::json!("1.21.11"));
        inputs.insert("eulaAccepted".to_string(), serde_json::json!(true));
        if let Some(filename) = jar_filename {
            inputs.insert(JAR_FILENAME_KEY.to_string(), serde_json::json!(filename));
        }
        inputs
    }

    #[test]
    fn render_runtime_config_rejects_a_declined_eula() {
        let blueprint = PaperBlueprint::new();
        let mut inputs = accepted_inputs(Some("paper-1.21.11-132.jar"));
        inputs.insert("eulaAccepted".to_string(), serde_json::json!(false));
        assert!(blueprint.render_runtime_config(&inputs).is_err());
    }

    #[test]
    fn render_runtime_config_rejects_a_missing_eula_field() {
        let blueprint = PaperBlueprint::new();
        let mut inputs = accepted_inputs(Some("paper-1.21.11-132.jar"));
        inputs.remove("eulaAccepted");
        assert!(blueprint.render_runtime_config(&inputs).is_err());
    }

    #[test]
    fn render_runtime_config_rejects_a_missing_jar_filename() {
        let blueprint = PaperBlueprint::new();
        let inputs = accepted_inputs(None);
        assert!(blueprint.render_runtime_config(&inputs).is_err());
    }

    #[test]
    fn render_runtime_config_builds_command_and_args_with_the_downloaded_jar() {
        let blueprint = PaperBlueprint::new();
        let inputs = accepted_inputs(Some("paper-1.21.11-132.jar"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "command": "java", "args": ["-jar", "paper-1.21.11-132.jar", "nogui"] }));
    }

    #[test]
    fn render_runtime_config_places_jvm_args_before_jar_and_program_args_after() {
        let blueprint = PaperBlueprint::new();
        let mut inputs = accepted_inputs(Some("paper-1.21.11-132.jar"));
        inputs.insert("jvmArgs".to_string(), serde_json::json!(["-Xmx4G"]));
        inputs.insert("programArgs".to_string(), serde_json::json!([]));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "command": "java", "args": ["-Xmx4G", "-jar", "paper-1.21.11-132.jar"] }));
    }

    #[tokio::test]
    async fn provision_downloads_a_real_jar_and_writes_the_eula_locally() {
        let blueprint = PaperBlueprint::new();
        let inputs = accepted_inputs(None);
        let working_directory = std::env::temp_dir().join(format!("vibessh-paper-provision-test-{}", uuid::Uuid::new_v4()));
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
        assert!(working_directory.join("eula.txt").is_file());
        let eula_contents = tokio::fs::read_to_string(working_directory.join("eula.txt")).await.unwrap();
        assert_eq!(eula_contents, "eula=true\n");

        tokio::fs::remove_dir_all(&working_directory).await.ok();
    }

    #[tokio::test]
    async fn provision_rejects_a_declined_eula_without_downloading_anything() {
        let blueprint = PaperBlueprint::new();
        let mut inputs = accepted_inputs(None);
        inputs.insert("eulaAccepted".to_string(), serde_json::json!(false));
        let working_directory = std::env::temp_dir().join(format!("vibessh-paper-provision-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&working_directory).await.unwrap();
        let context = ProvisionContext { working_directory: working_directory.to_str().unwrap(), connection: None };

        assert!(blueprint.provision(&inputs, &context).await.is_err());
        assert!(tokio::fs::read_dir(&working_directory).await.unwrap().next_entry().await.unwrap().is_none());

        tokio::fs::remove_dir_all(&working_directory).await.ok();
    }
}
