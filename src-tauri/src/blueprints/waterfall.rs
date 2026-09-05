use std::collections::HashMap;

use crate::errors::{AppError, AppResult};
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, DefaultPort, KnownFile, PortProtocol, RuntimeType};
use crate::services::latest_waterfall_build;

use super::{JAVA_PATH_KEY, ensure_java_for, render_java_config, text_input, text_list_input, validate_inputs, BlueprintHandler, ProvisionContext};
// The one shared implementation - every module that builds a remote
// command used to carry its own byte-identical copy of this.
use crate::ssh::command::quote as shell_quote;

/// Not a user-facing wizard field - see `paper::JAR_FILENAME_KEY`'s own
/// doc comment for the full reasoning (identical here).
const JAR_FILENAME_KEY: &str = "__jarFilename";

/// A BungeeCord-based Minecraft proxy - the jar is downloaded automatically
/// from PaperMC (Waterfall is one of the projects behind PaperMC's own Fill
/// API, same as Paper/Velocity) for the chosen Waterfall version. No EULA,
/// same reasoning as `VelocityBlueprint`.
pub struct WaterfallBlueprint {
    definition: Blueprint,
}

impl WaterfallBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "waterfall".to_string(),
                name: "Waterfall".to_string(),
                description: "A BungeeCord-based Minecraft proxy - the jar is downloaded and kept up to date automatically.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                // Docker-only since Etap M1 - see `PaperBlueprint`'s own doc
                // comment for the full reasoning (identical here).
                // Local as well as Docker. Locally there is no image to bring a JVM,
                // so the provision step finds or downloads one - which is what lets a
                // Minecraft server run on a machine with nothing installed on it.
                supported_runtime_types: vec![RuntimeType::Docker, RuntimeType::LocalProcess],
                features: vec![BlueprintFeature::Console, BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck, BlueprintFeature::Databases, BlueprintFeature::Files],
                fields: vec![
                    BlueprintField {
                        key: "waterfallVersion".to_string(),
                        label: "Waterfall version".to_string(),
                        field_type: BlueprintFieldType::PapermcVersion,
                        required: true,
                        default_value: None,
                        help_text: Some("The matching Waterfall jar is downloaded automatically.".to_string()),
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
                known_files: vec![KnownFile { path: "config.yml".to_string(), label: "config.yml".to_string() }],
                default_ports: vec![DefaultPort {
                    name: "Proxy".to_string(),
                    protocol: PortProtocol::Tcp,
                    internal_port: 25577,
                    external_port: 25577,
                }],
                is_builtin: true,
            },
        }
    }
}

impl Default for WaterfallBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for WaterfallBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;

        let jar_filename = inputs
            .get(JAR_FILENAME_KEY)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AppError::Internal("Waterfall's jar filename wasn't set by provision() before rendering".into()))?;

        let java_version = text_input(inputs, &self.definition, "javaVersion")?;
        let jvm_args = text_list_input(inputs, &self.definition, "jvmArgs")?;
        let program_args = text_list_input(inputs, &self.definition, "programArgs")?;

        // Recorded by `provision` when this Application runs as a local
        // process: the path to a JVM on this machine. Absent for Docker,
        // where the image brings its own.
        let java_path = inputs.get(JAVA_PATH_KEY).and_then(serde_json::Value::as_str);
        render_java_config(&java_version, jvm_args, jar_filename.to_string(), program_args, java_path, Some("end"))
    }

    async fn provision(
        &self,
        inputs: &HashMap<String, serde_json::Value>,
        context: &ProvisionContext<'_>,
    ) -> AppResult<HashMap<String, serde_json::Value>> {
        validate_inputs(&self.definition, inputs)?;
        let version = text_input(inputs, &self.definition, "waterfallVersion")?;
        if version.trim().is_empty() {
            return Err(AppError::InvalidInput("a Waterfall version is required".into()));
        }

        let build = latest_waterfall_build(&version).await?;
        download_file(context, &build.url, &build.filename).await?;

        let mut discovered = HashMap::new();
        discovered.insert(JAR_FILENAME_KEY.to_string(), serde_json::Value::String(build.filename));
        ensure_java_for(context, &text_input(inputs, &self.definition, "javaVersion")?, &mut discovered).await?;
        Ok(discovered)
    }
}

/// Same reasoning as `velocity::download_file` (identical here).
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


#[cfg(test)]
mod tests {
    use super::*;

    fn inputs_with_jar(jar_filename: Option<&str>) -> HashMap<String, serde_json::Value> {
        let mut inputs = HashMap::new();
        inputs.insert("waterfallVersion".to_string(), serde_json::json!("1.20"));
        if let Some(filename) = jar_filename {
            inputs.insert(JAR_FILENAME_KEY.to_string(), serde_json::json!(filename));
        }
        inputs
    }

    #[test]
    fn declares_a_default_published_port_so_a_fresh_proxy_is_reachable_without_manual_setup() {
        let blueprint = WaterfallBlueprint::new();
        assert_eq!(blueprint.blueprint().default_ports.len(), 1);
        let port = &blueprint.blueprint().default_ports[0];
        assert_eq!(port.internal_port, 25577);
        assert_eq!(port.external_port, 25577);
        assert_eq!(port.protocol, PortProtocol::Tcp);
    }

    #[test]
    fn render_runtime_config_rejects_a_missing_jar_filename() {
        let blueprint = WaterfallBlueprint::new();
        let inputs = inputs_with_jar(None);
        assert!(blueprint.render_runtime_config(&inputs).is_err());
    }

    #[test]
    fn render_runtime_config_builds_a_docker_image_and_command_with_the_downloaded_jar() {
        let blueprint = WaterfallBlueprint::new();
        let inputs = inputs_with_jar(Some("waterfall-1.20-497.jar"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "image": "eclipse-temurin:21-jre", "command": ["java", "-jar", "waterfall-1.20-497.jar"], "runAsDedicatedUser": true }));
    }

    #[tokio::test]
    async fn provision_downloads_a_real_jar_locally() {
        let blueprint = WaterfallBlueprint::new();
        let inputs = inputs_with_jar(None);
        let working_directory = std::env::temp_dir().join(format!("vibessh-waterfall-provision-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&working_directory).await.unwrap();
        let context = ProvisionContext { working_directory: working_directory.to_str().unwrap(), connection: None, runtime_type: RuntimeType::Docker, java_root: std::path::Path::new("") };

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
