use std::collections::HashMap;

use crate::errors::{AppError, AppResult};
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, DefaultPort, KnownFile, PortProtocol, RuntimeType};
use crate::services::latest_purpur_build;

use super::{bool_input, render_java_docker_config, text_input, text_list_input, validate_inputs, BlueprintHandler, ProvisionContext};

/// Not a user-facing wizard field - see `paper::JAR_FILENAME_KEY`'s own
/// doc comment for the full reasoning (identical here).
const JAR_FILENAME_KEY: &str = "__jarFilename";

/// A Paper fork with extra performance tweaks and gameplay/config options -
/// same idea as `PaperBlueprint` (jar downloaded and kept up to date
/// automatically, EULA required), but built from Purpur's own independent
/// build API (`purpur_service`) rather than PaperMC's, since Purpur is
/// distributed separately from PaperMC's own projects.
pub struct PurpurBlueprint {
    definition: Blueprint,
}

impl PurpurBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "purpur".to_string(),
                name: "Purpur".to_string(),
                description: "A Paper fork with extra performance and gameplay options - the server jar is downloaded and kept up to date automatically.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                // Docker-only since Etap M1 - see `PaperBlueprint`'s own doc
                // comment for the full reasoning (identical here).
                supported_runtime_types: vec![RuntimeType::Docker],
                features: vec![BlueprintFeature::Console, BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck, BlueprintFeature::Databases, BlueprintFeature::Files],
                fields: vec![
                    BlueprintField {
                        key: "purpurVersion".to_string(),
                        label: "Minecraft version".to_string(),
                        field_type: BlueprintFieldType::PapermcVersion,
                        required: true,
                        default_value: None,
                        help_text: Some("The matching Purpur server jar is downloaded automatically.".to_string()),
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
                        key: "javaVersion".to_string(),
                        label: "Java version".to_string(),
                        field_type: BlueprintFieldType::Text,
                        required: false,
                        default_value: Some(serde_json::Value::String("21".to_string())),
                        help_text: Some("Any Java major version available as an eclipse-temurin image, e.g. 25, 21, 17, or 11 - selects the matching Docker image.".to_string()),
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
                known_files: vec![
                    KnownFile { path: "server.properties".to_string(), label: "server.properties".to_string() },
                    KnownFile { path: "bukkit.yml".to_string(), label: "bukkit.yml".to_string() },
                    KnownFile { path: "spigot.yml".to_string(), label: "spigot.yml".to_string() },
                    KnownFile { path: "purpur.yml".to_string(), label: "purpur.yml".to_string() },
                    KnownFile { path: "config/paper-global.yml".to_string(), label: "paper-global.yml".to_string() },
                    KnownFile { path: "config/paper-world-defaults.yml".to_string(), label: "paper-world-defaults.yml".to_string() },
                ],
                default_ports: vec![DefaultPort {
                    name: "Minecraft".to_string(),
                    protocol: PortProtocol::Tcp,
                    internal_port: 25565,
                    external_port: 25565,
                }],
                is_builtin: true,
            },
        }
    }
}

impl Default for PurpurBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

/// Same reasoning as `paper::require_eula_accepted` (identical here).
fn require_eula_accepted(definition: &Blueprint, inputs: &HashMap<String, serde_json::Value>) -> AppResult<()> {
    if !bool_input(inputs, definition, "eulaAccepted")? {
        return Err(AppError::InvalidInput("the Minecraft EULA must be accepted to create a Purpur application".into()));
    }
    Ok(())
}

#[async_trait::async_trait]
impl BlueprintHandler for PurpurBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        require_eula_accepted(&self.definition, inputs)?;

        let jar_filename = inputs
            .get(JAR_FILENAME_KEY)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AppError::Internal("Purpur's jar filename wasn't set by provision() before rendering".into()))?;

        let java_version = text_input(inputs, &self.definition, "javaVersion")?;
        let jvm_args = text_list_input(inputs, &self.definition, "jvmArgs")?;
        let program_args = text_list_input(inputs, &self.definition, "programArgs")?;

        Ok(render_java_docker_config(&java_version, jvm_args, jar_filename.to_string(), program_args))
    }

    async fn provision(
        &self,
        inputs: &HashMap<String, serde_json::Value>,
        context: &ProvisionContext<'_>,
    ) -> AppResult<HashMap<String, serde_json::Value>> {
        validate_inputs(&self.definition, inputs)?;
        require_eula_accepted(&self.definition, inputs)?;
        let version = text_input(inputs, &self.definition, "purpurVersion")?;
        if version.trim().is_empty() {
            return Err(AppError::InvalidInput("a Minecraft version is required".into()));
        }

        let build = latest_purpur_build(&version).await?;
        download_file(context, &build.url, &build.filename).await?;
        write_eula(context).await?;

        let mut discovered = HashMap::new();
        discovered.insert(JAR_FILENAME_KEY.to_string(), serde_json::Value::String(build.filename));
        Ok(discovered)
    }
}

/// Same reasoning as `paper::download_file` (identical here).
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
        // `sudo curl` - see `paper::download_file`'s own doc comment for
        // why (identical here).
        Some(connection) => {
            let path = format!("{}/{}", context.working_directory.trim_end_matches('/'), filename);
            let output = connection.execute_command(&format!("sudo curl -fsSL -o {} {}", shell_quote(&path), shell_quote(url))).await?;
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
    const EULA_CONTENT: &str = "eula=true\n";
    match &context.connection {
        None => {
            let path = std::path::Path::new(context.working_directory).join("eula.txt");
            tokio::fs::write(&path, EULA_CONTENT).await.map_err(|err| AppError::InvalidInput(format!("couldn't write eula.txt: {err}")))
        }
        // `sudo tee` - see `paper::write_eula`'s own doc comment for why
        // (identical here).
        Some(connection) => {
            let path = format!("{}/eula.txt", context.working_directory.trim_end_matches('/'));
            let output = connection.execute_command(&format!("printf '%s' {} | sudo tee {} >/dev/null", shell_quote(EULA_CONTENT), shell_quote(&path))).await?;
            if output.exit_code != 0 {
                let detail = output.stderr.trim();
                let detail = if detail.is_empty() { "couldn't write eula.txt".to_string() } else { detail.to_string() };
                return Err(AppError::Connection(format!("couldn't write eula.txt on the remote host: {detail}")));
            }
            Ok(())
        }
    }
}

/// POSIX single-quote shell escaping - duplicated across this crate per its
/// own small-helper convention; see `runtime::remote_process`'s copy for the
/// full reasoning.
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

    #[test]
    fn declares_a_default_published_port_so_a_fresh_server_is_reachable_without_manual_setup() {
        let blueprint = PurpurBlueprint::new();
        assert_eq!(blueprint.blueprint().default_ports.len(), 1);
        let port = &blueprint.blueprint().default_ports[0];
        assert_eq!(port.internal_port, 25565);
        assert_eq!(port.external_port, 25565);
        assert_eq!(port.protocol, PortProtocol::Tcp);
    }

    fn accepted_inputs(jar_filename: Option<&str>) -> HashMap<String, serde_json::Value> {
        let mut inputs = HashMap::new();
        inputs.insert("purpurVersion".to_string(), serde_json::json!("1.21.4"));
        inputs.insert("eulaAccepted".to_string(), serde_json::json!(true));
        if let Some(filename) = jar_filename {
            inputs.insert(JAR_FILENAME_KEY.to_string(), serde_json::json!(filename));
        }
        inputs
    }

    #[test]
    fn render_runtime_config_rejects_a_declined_eula() {
        let blueprint = PurpurBlueprint::new();
        let mut inputs = accepted_inputs(Some("purpur-1.21.4-2416.jar"));
        inputs.insert("eulaAccepted".to_string(), serde_json::json!(false));
        assert!(blueprint.render_runtime_config(&inputs).is_err());
    }

    #[test]
    fn render_runtime_config_rejects_a_missing_jar_filename() {
        let blueprint = PurpurBlueprint::new();
        let inputs = accepted_inputs(None);
        assert!(blueprint.render_runtime_config(&inputs).is_err());
    }

    #[test]
    fn render_runtime_config_builds_a_docker_image_and_command_with_the_downloaded_jar() {
        let blueprint = PurpurBlueprint::new();
        let inputs = accepted_inputs(Some("purpur-1.21.4-2416.jar"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(
            config,
            serde_json::json!({ "image": "eclipse-temurin:21-jre-alpine", "command": ["java", "-jar", "purpur-1.21.4-2416.jar", "nogui"], "runAsDedicatedUser": true })
        );
    }

    #[tokio::test]
    async fn provision_downloads_a_real_jar_and_writes_the_eula_locally() {
        let blueprint = PurpurBlueprint::new();
        let inputs = accepted_inputs(None);
        let working_directory = std::env::temp_dir().join(format!("vibessh-purpur-provision-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&working_directory).await.unwrap();
        let context = ProvisionContext { working_directory: working_directory.to_str().unwrap(), connection: None };

        let result = blueprint.provision(&inputs, &context).await;
        let Ok(discovered) = result else {
            eprintln!("skipping: purpurmc.org unreachable from this environment ({:?})", result.err());
            tokio::fs::remove_dir_all(&working_directory).await.ok();
            return;
        };

        let jar_filename = discovered[JAR_FILENAME_KEY].as_str().unwrap().to_string();
        assert!(working_directory.join(&jar_filename).is_file());
        assert!(working_directory.join("eula.txt").is_file());

        tokio::fs::remove_dir_all(&working_directory).await.ok();
    }

    #[tokio::test]
    async fn provision_rejects_a_declined_eula_without_downloading_anything() {
        let blueprint = PurpurBlueprint::new();
        let mut inputs = accepted_inputs(None);
        inputs.insert("eulaAccepted".to_string(), serde_json::json!(false));
        let working_directory = std::env::temp_dir().join(format!("vibessh-purpur-provision-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&working_directory).await.unwrap();
        let context = ProvisionContext { working_directory: working_directory.to_str().unwrap(), connection: None };

        assert!(blueprint.provision(&inputs, &context).await.is_err());
        assert!(tokio::fs::read_dir(&working_directory).await.unwrap().next_entry().await.unwrap().is_none());

        tokio::fs::remove_dir_all(&working_directory).await.ok();
    }
}
