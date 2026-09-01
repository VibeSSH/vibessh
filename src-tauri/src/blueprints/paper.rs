use std::collections::HashMap;

use crate::errors::{AppError, AppResult};
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, DefaultPort, KnownFile, PortProtocol, RuntimeType};
use crate::services::latest_paper_build;

use super::{bool_input, render_java_docker_config, text_input, text_list_input, validate_inputs, BlueprintHandler, ProvisionContext};
// The one shared implementation - every module that builds a remote
// command used to carry its own byte-identical copy of this.
use crate::ssh::command::quote as shell_quote;

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
                // Docker-only since Etap M1 (mandatory Docker isolation) -
                // see `runtime::docker`'s own module doc comment for what
                // that gets this Egg (a real bind-mounted, persistent
                // `working_directory`) and `blueprints::mod`'s
                // `render_java_docker_config` for how "Java version" below
                // maps to a real image instead of a host-installed JDK path.
                supported_runtime_types: vec![RuntimeType::Docker],
                features: vec![BlueprintFeature::Console, BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck, BlueprintFeature::Databases, BlueprintFeature::Files],
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
        //
        // `sudo curl`, not a plain `curl` - same reasoning as
        // `runtime::docker::DockerConsole::write`'s own `sudo tee`: a
        // `run_as_dedicated_user` Application's whole `working_directory`
        // gets `chown -R`'d to that Application's own dedicated account on
        // every start (`ensure_working_directory_owned_by_dedicated_user`),
        // so a re-provision (e.g. changing the Minecraft version after the
        // Application has already been started once) would otherwise fail
        // to write here as the plain SSH login user - `curl: (23) Failure
        // writing output to destination`, not an obviously
        // permissions-shaped error. `sudo` sidesteps the ownership question
        // entirely; the next start re-chowns the freshly downloaded jar
        // along with everything else already in the working directory.
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
        // `sudo tee`, not the plain SFTP `write_file` - same
        // dedicated-user-ownership reason `download_file`'s own `sudo curl`
        // (just above) and `runtime::docker::DockerConsole::write`'s `sudo
        // tee` both already need: a re-provision (changing the Minecraft
        // version after the Application has already been started once)
        // would otherwise fail to overwrite a working directory this
        // connection's own login user no longer owns.
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_a_default_published_port_so_a_fresh_server_is_reachable_without_manual_setup() {
        let blueprint = PaperBlueprint::new();
        assert_eq!(blueprint.blueprint().default_ports.len(), 1);
        let port = &blueprint.blueprint().default_ports[0];
        assert_eq!(port.internal_port, 25565);
        assert_eq!(port.external_port, 25565);
        assert_eq!(port.protocol, PortProtocol::Tcp);
    }

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
    fn render_runtime_config_builds_a_docker_image_and_command_with_the_downloaded_jar() {
        let blueprint = PaperBlueprint::new();
        let inputs = accepted_inputs(Some("paper-1.21.11-132.jar"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(
            config,
            serde_json::json!({ "image": "eclipse-temurin:21-jre-alpine", "command": ["java", "-jar", "paper-1.21.11-132.jar", "nogui"], "runAsDedicatedUser": true })
        );
    }

    #[test]
    fn render_runtime_config_places_jvm_args_before_jar_and_program_args_after() {
        let blueprint = PaperBlueprint::new();
        let mut inputs = accepted_inputs(Some("paper-1.21.11-132.jar"));
        inputs.insert("jvmArgs".to_string(), serde_json::json!(["-Xmx4G"]));
        inputs.insert("programArgs".to_string(), serde_json::json!([]));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config["command"], serde_json::json!(["java", "-Xmx4G", "-jar", "paper-1.21.11-132.jar"]));
    }

    #[test]
    fn render_runtime_config_uses_the_chosen_java_version_as_the_image_tag() {
        let blueprint = PaperBlueprint::new();
        let mut inputs = accepted_inputs(Some("paper-1.21.11-132.jar"));
        inputs.insert("javaVersion".to_string(), serde_json::json!("17"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config["image"], serde_json::json!("eclipse-temurin:17-jre-alpine"));
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
