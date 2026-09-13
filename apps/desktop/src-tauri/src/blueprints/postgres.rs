use std::collections::HashMap;

use crate::errors::AppResult;
use crate::models::{Blueprint, BlueprintCommandConsole, BlueprintFeature, BlueprintField, BlueprintFieldType, RuntimeType};

use super::{text_input, validate_inputs, BlueprintHandler};

/// A self-hosted PostgreSQL server.
///
/// **Why the data directory is the user's problem here, unlike MariaDB's.**
/// `MariaDbBlueprint` keeps data in the Application's working directory with
/// `--datadir=.`, a command-line flag this blueprint can render. Postgres has
/// no equivalent: the official image's entrypoint runs `initdb` against
/// `$PGDATA` *before* the server is started, so `-c data_directory=...` moves
/// only where the server looks, not where it was initialised - the server
/// would then start against an empty directory. `PGDATA` is an environment
/// variable, and a blueprint's fields and the Environment tab are two
/// separate channels with no bridge between them (the same split
/// `RedisBlueprint` documents from the other side).
///
/// So the description asks for `PGDATA=./pgdata` explicitly, and says what
/// happens without it, because the failure is silent and expensive: the
/// database initialises inside the container instead, works perfectly, and
/// is lost the next time the container is recreated - which an edit to the
/// image or the command does on its own.
///
/// `./pgdata` rather than `.`: `initdb` refuses a directory that is not
/// empty, and the working directory of an Application generally is not. The
/// relative path resolves because the Docker runtime mounts
/// `working_directory` at the same path inside the container and makes it the
/// container's cwd - the trick `MariaDbBlueprint` documents in full.
///
/// No `default_ports`, for the reason `MariaDbBlueprint` gives: every entry
/// there is published as `Public` on creation with no review step, which is
/// the wrong default for a database. The user adds one on the Ports tab,
/// ideally VibeNetwork-only.
pub struct PostgresBlueprint {
    definition: Blueprint,
}

impl PostgresBlueprint {
    pub fn new() -> Self {
        Self {
            definition: Blueprint {
                id: "postgres".to_string(),
                name: "PostgreSQL".to_string(),
                description: "A self-hosted PostgreSQL database server - data is stored in this Application's own working directory. Set POSTGRES_PASSWORD and PGDATA=./pgdata in the Environment tab before starting it: without PGDATA the database is created inside the container instead, and recreating the container loses it.".to_string(),
                schema_version: 1,
                blueprint_version: 1,
                supported_runtime_types: vec![RuntimeType::Docker],
                features: vec![BlueprintFeature::Logs, BlueprintFeature::Environment, BlueprintFeature::Ports, BlueprintFeature::HealthCheck, BlueprintFeature::Files],
                fields: vec![BlueprintField {
                    key: "postgresVersion".to_string(),
                    label: "PostgreSQL version".to_string(),
                    field_type: BlueprintFieldType::Text,
                    required: false,
                    default_value: Some(serde_json::Value::String("17".to_string())),
                    help_text: Some("A Docker Hub tag, e.g. 17, 16, 15, or 17-alpine.".to_string()),
                }],
                known_files: vec![],
                default_ports: vec![],
                connects_to: None,
                // `psql`, inside the container, over the local socket the
                // image's own `pg_hba.conf` trusts. The account and database
                // come from the environment the image already holds, so
                // nothing about them is sent from this host - the same
                // reasoning `RedisBlueprint` records for its password.
                //
                // The statement goes in on stdin rather than through `-c`,
                // so `psql` does its own parsing and a query containing
                // quotes survives the trip.
                command_console: Some(BlueprintCommandConsole {
                    shell: r#"printf '%s\n' "$1" | psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER:-postgres}" -d "${POSTGRES_DB:-${POSTGRES_USER:-postgres}}""#.to_string(),
                    placeholder: "SELECT version();".to_string(),
                }),
                is_builtin: true,
            },
        }
    }
}

impl Default for PostgresBlueprint {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BlueprintHandler for PostgresBlueprint {
    fn blueprint(&self) -> &Blueprint {
        &self.definition
    }

    fn render_runtime_config(&self, inputs: &HashMap<String, serde_json::Value>) -> AppResult<serde_json::Value> {
        validate_inputs(&self.definition, inputs)?;
        let version = text_input(inputs, &self.definition, "postgresVersion")?;
        let image = format!("postgres:{}", version.trim());
        Ok(serde_json::json!({ "image": image }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_versioned_image_and_leaves_the_images_own_entrypoint_alone() {
        let blueprint = PostgresBlueprint::new();
        let config = blueprint.render_runtime_config(&HashMap::new()).unwrap();
        // No `command`: the image's entrypoint is what runs `initdb` and then
        // starts the server, and overriding it is how that gets skipped.
        assert_eq!(config, serde_json::json!({ "image": "postgres:17" }));
    }

    #[test]
    fn a_custom_version_overrides_the_default_image_tag() {
        let blueprint = PostgresBlueprint::new();
        let mut inputs = HashMap::new();
        inputs.insert("postgresVersion".to_string(), serde_json::json!("16-alpine"));
        let config = blueprint.render_runtime_config(&inputs).unwrap();
        assert_eq!(config["image"], serde_json::json!("postgres:16-alpine"));
    }

    #[test]
    fn declares_no_default_ports_so_nothing_is_published_publicly_without_review() {
        let blueprint = PostgresBlueprint::new();
        assert!(blueprint.blueprint().default_ports.is_empty());
    }

    /// The one setting whose absence loses data silently. It cannot be
    /// rendered from here - see this blueprint's own doc comment - so saying
    /// so is the whole mitigation, and it has to survive an edit to the text.
    #[test]
    fn the_description_asks_for_the_data_directory_and_says_why() {
        let blueprint = PostgresBlueprint::new();
        let description = &blueprint.blueprint().description;
        assert!(description.contains("PGDATA=./pgdata"), "{description}");
        assert!(description.contains("POSTGRES_PASSWORD"), "{description}");
        assert!(description.contains("recreating the container loses it"), "{description}");
    }

    /// A database ignores stdin, so its console has to run a client - and
    /// that client must not be handed the statement as a shell argument.
    #[test]
    fn the_console_feeds_the_statement_to_psql_on_stdin() {
        let console = PostgresBlueprint::new().blueprint().command_console.clone().expect("a database needs one");
        assert!(console.shell.contains("| psql"), "{}", console.shell);
        assert!(console.shell.contains("printf"), "{}", console.shell);
    }
}
