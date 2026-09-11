use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{JAVA_PATH_KEY, render_java_config, text_input, text_list_input, validate_inputs, BlueprintHandler};

/// Runs a `.jar` file with a JVM - the base every Java-based server
/// (Minecraft and friends) builds on; Paper/Velocity (Phase 8/9) will be
/// their own, more specific blueprints layered on the same idea, not a
/// special-cased branch of this one.
pub struct GenericJavaBlueprint {
    definition: Blueprint,
}

impl GenericJavaBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "generic-java".to_string(),
                name: "Generic Java Application".to_string(),
                description: "Runs a .jar file with a JVM.".to_string(),
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
                        key: "javaVersion".to_string(),
                        label: "Java version".to_string(),
                        field_type: BlueprintFieldType::Text,
                        required: false,
                        default_value: Some(serde_json::Value::String("21".to_string())),
                        help_text: Some("Any Java major version available as an eclipse-temurin image, e.g. 25, 21, 17, or 11 - selects the matching Docker image.".to_string()),
                    },
                    BlueprintField {
                        key: "jarPath".to_string(),
                        label: "Jar file".to_string(),
                        field_type: BlueprintFieldType::Path,
                        required: true,
                        default_value: None,
                        help_text: Some("Path to the .jar file, relative to the application's working directory.".to_string()),
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
                        default_value: Some(serde_json::json!([])),
                        help_text: Some("Arguments passed to the jar itself, after its own -jar entry.".to_string()),
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

impl Default for GenericJavaBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for GenericJavaBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let java_version = text_input(inputs, &self.definition, "javaVersion")?;
        let jar_path = text_input(inputs, &self.definition, "jarPath")?;
        let jvm_args = text_list_input(inputs, &self.definition, "jvmArgs")?;
        let program_args = text_list_input(inputs, &self.definition, "programArgs")?;

        // Recorded by `provision` when this Application runs as a local
        // process: the path to a JVM on this machine. Absent for Docker,
        // where the image brings its own.
        let java_path = inputs.get(JAVA_PATH_KEY).and_then(serde_json::Value::as_str);
        render_java_config(&java_version, jvm_args, jar_path, program_args, java_path, None)
    }
}

#[cfg(test)]
mod tests {
    /// The failure this guards against, seen in the wild: a start script
    /// pasted into the JVM arguments field. It splits on whitespace, so the
    /// shebang becomes the first argument, and because JVM arguments sit
    /// before `-jar` Java reads it as a main class - then the container
    /// restarts forever on `Could not find or load main class #!.bin.bash`,
    /// an error naming a class nobody ever wrote.
    #[test]
    fn a_pasted_shell_script_is_refused_rather_than_run() {
        let mut inputs = std::collections::HashMap::new();
        inputs.insert("jarPath".to_string(), serde_json::json!("server.jar"));
        inputs.insert("jvmArgs".to_string(), serde_json::json!(["#!/bin/bash", "java", "-Xmx4G"]));

        let result = GenericJavaBlueprint::new().render_runtime_config(&inputs);

        match result {
            Err(crate::errors::AppError::InvalidInput(message)) => {
                assert!(message.contains("shell script"), "{message}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }


    use super::*;

    #[test]
    fn renders_jvm_args_then_dash_jar_then_the_jar_then_program_args() {
        let blueprint = GenericJavaBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("jarPath".to_string(), serde_json::json!("server.jar"));
        inputs.insert("jvmArgs".to_string(), serde_json::json!(["-Xmx2G", "-Xms1G"]));
        inputs.insert("programArgs".to_string(), serde_json::json!(["--nogui"]));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(
            config,
            serde_json::json!({ "image": "eclipse-temurin:21-jre", "command": ["java", "-Xmx2G", "-Xms1G", "-jar", "server.jar", "--nogui"], "runAsDedicatedUser": true })
        );
    }

    #[test]
    fn java_version_defaults_to_21_when_omitted() {
        let blueprint = GenericJavaBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("jarPath".to_string(), serde_json::json!("server.jar"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "image": "eclipse-temurin:21-jre", "command": ["java", "-jar", "server.jar"], "runAsDedicatedUser": true }));
    }

    #[test]
    fn a_custom_java_version_overrides_the_default_image_tag() {
        let blueprint = GenericJavaBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("javaVersion".to_string(), serde_json::json!("17"));
        inputs.insert("jarPath".to_string(), serde_json::json!("server.jar"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config["image"], serde_json::json!("eclipse-temurin:17-jre"));
    }

    #[test]
    fn rejects_a_missing_jar_path() {
        let blueprint = GenericJavaBlueprint::new();
        assert!(blueprint.render_runtime_config(&HashMap::new()).is_err());
    }
}
