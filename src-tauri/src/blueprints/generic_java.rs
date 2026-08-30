use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, text_list_input, validate_inputs, BlueprintHandler};

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
                supported_runtime_types: vec![RuntimeType::LocalProcess, RuntimeType::RemoteProcess, RuntimeType::Systemd],
                features: vec![BlueprintFeature::Console, BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck],
                fields: vec![
                    BlueprintField {
                        key: "javaBinary".to_string(),
                        label: "Java version".to_string(),
                        field_type: BlueprintFieldType::JavaVersion,
                        required: false,
                        default_value: Some(serde_json::Value::String("java".to_string())),
                        help_text: Some("Detected Java installations - pick one, or enter a path yourself.".to_string()),
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
        let java_binary = text_input(inputs, &self.definition, "javaBinary")?;
        let jar_path = text_input(inputs, &self.definition, "jarPath")?;
        let jvm_args = text_list_input(inputs, &self.definition, "jvmArgs")?;
        let program_args = text_list_input(inputs, &self.definition, "programArgs")?;

        let mut args = jvm_args;
        args.push("-jar".to_string());
        args.push(jar_path);
        args.extend(program_args);

        Ok(serde_json::json!({ "command": java_binary, "args": args }))
    }
}

#[cfg(test)]
mod tests {
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
            serde_json::json!({ "command": "java", "args": ["-Xmx2G", "-Xms1G", "-jar", "server.jar", "--nogui"] })
        );
    }

    #[test]
    fn java_binary_defaults_to_plain_java_when_omitted() {
        let blueprint = GenericJavaBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("jarPath".to_string(), serde_json::json!("server.jar"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config, serde_json::json!({ "command": "java", "args": ["-jar", "server.jar"] }));
    }

    #[test]
    fn a_custom_java_binary_overrides_the_default() {
        let blueprint = GenericJavaBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("javaBinary".to_string(), serde_json::json!("/opt/jdk21/bin/java"));
        inputs.insert("jarPath".to_string(), serde_json::json!("server.jar"));

        let config = blueprint.render_runtime_config(&inputs).unwrap();

        assert_eq!(config["command"], serde_json::json!("/opt/jdk21/bin/java"));
    }

    #[test]
    fn rejects_a_missing_jar_path() {
        let blueprint = GenericJavaBlueprint::new();
        assert!(blueprint.render_runtime_config(&HashMap::new()).is_err());
    }
}
