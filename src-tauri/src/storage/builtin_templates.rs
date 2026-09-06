//! Templates that ship with VibeSSH.
//!
//! **Why these exist.** A blueprint says what image to run; it does not say
//! which environment variables that image refuses to start without. MariaDB
//! is the plain example: with no `MYSQL_ROOT_PASSWORD` the container exits on
//! first boot with a message about initialization, and nothing in the wizard
//! ever mentioned the variable's name. phpMyAdmin is the same story one step
//! along - it starts fine and then cannot reach anything.
//!
//! So a built-in template is not a shortcut for an expert; it is the list of
//! variables somebody would otherwise have to find in Docker Hub
//! documentation, put in front of them with the values they can fill in. The
//! secret ones arrive empty on purpose, exactly like a saved template's do -
//! see `TemplateEnvironmentVariable`.
//!
//! They are read-only: `save_template` and `delete_template` both refuse
//! these ids, so a built-in cannot be edited into something that no longer
//! matches what this file says, and cannot be deleted and then missed.

use uuid::Uuid;

use crate::models::{ApplicationTemplate, RuntimeType, TemplateEnvironmentVariable};

/// Fixed so a template keeps its identity across releases - a fresh uuid per
/// launch would make every built-in look like a new template to anything that
/// remembers them, and would let a stale copy of one linger in a saved file
/// under a different id.
const MARIADB_ID: Uuid = Uuid::from_u128(0x7b1d_0001_0000_4000_8000_5642_4942_4553);
const PHPMYADMIN_ID: Uuid = Uuid::from_u128(0x7b1d_0002_0000_4000_8000_5642_4942_4553);
const MONGODB_ID: Uuid = Uuid::from_u128(0x7b1d_0004_0000_4000_8000_5642_4942_4553);
const PHPMYADMIN_ARBITRARY_ID: Uuid = Uuid::from_u128(0x7b1d_0003_0000_4000_8000_5642_4942_4553);

fn plain(key: &str, value: &str) -> TemplateEnvironmentVariable {
    TemplateEnvironmentVariable { key: key.to_string(), value: value.to_string(), is_secret: false }
}

/// A password the wizard will ask for. Empty, like every secret row - the
/// value never travels in a template.
fn secret(key: &str) -> TemplateEnvironmentVariable {
    TemplateEnvironmentVariable { key: key.to_string(), value: String::new(), is_secret: true }
}

/// The templates every install has, newest concern first.
///
/// `created_at` is the Unix epoch rather than "now": these were not created
/// during this launch, and stamping them with the current time would sort
/// them above everything the user actually saved.
pub fn builtin_templates() -> Vec<ApplicationTemplate> {
    let epoch = chrono::DateTime::from_timestamp(0, 0).expect("the epoch is a valid timestamp");
    vec![
        ApplicationTemplate {
            id: MARIADB_ID,
            name: "MariaDB with a root password".to_string(),
            blueprint_id: "mariadb".to_string(),
            runtime_type: RuntimeType::Docker,
            field_values: serde_json::json!({ "mariadbVersion": "11" }),
            // The four the official image reads on first boot. The root
            // password is the one it will not start without; the other three
            // create a database and an account for it in the same step, which
            // is otherwise a `mysql` shell somebody has to know their way
            // around.
            environment: vec![
                secret("MYSQL_ROOT_PASSWORD"),
                plain("MYSQL_DATABASE", "app"),
                plain("MYSQL_USER", "app"),
                secret("MYSQL_PASSWORD"),
            ],
            created_at: epoch,
            is_builtin: true,
        },
        ApplicationTemplate {
            id: PHPMYADMIN_ID,
            name: "phpMyAdmin for a MariaDB application".to_string(),
            blueprint_id: "phpmyadmin".to_string(),
            runtime_type: RuntimeType::Docker,
            field_values: serde_json::json!({ "phpMyAdminVersion": "latest" }),
            // No PMA_HOST here on purpose. The host is the target's network
            // alias, which no template can know - the wizard's own database
            // picker fills it in, along with the connection that makes it
            // resolvable. The port is here because it is the one half that is
            // the same everywhere and worth seeing before creating anything.
            environment: vec![plain("PMA_PORT", "3306")],
            created_at: epoch,
            is_builtin: true,
        },
        ApplicationTemplate {
            id: MONGODB_ID,
            name: "MongoDB with an administrator account".to_string(),
            blueprint_id: "mongodb".to_string(),
            runtime_type: RuntimeType::Docker,
            field_values: serde_json::json!({ "mongodbVersion": "8" }),
            // Both, always, and never one without the other: the official
            // image creates the administrator account only when it has both,
            // and setting them is also what makes it start the server with
            // authentication enabled. One alone leaves a database that
            // accepts anyone who can reach it.
            environment: vec![plain("MONGO_INITDB_ROOT_USERNAME", "root"), secret("MONGO_INITDB_ROOT_PASSWORD")],
            created_at: epoch,
            is_builtin: true,
        },
        ApplicationTemplate {
            id: PHPMYADMIN_ARBITRARY_ID,
            name: "phpMyAdmin for any server".to_string(),
            blueprint_id: "phpmyadmin".to_string(),
            runtime_type: RuntimeType::Docker,
            field_values: serde_json::json!({ "phpMyAdminVersion": "latest" }),
            // The case the picker deliberately does not cover: a database
            // VibeSSH does not manage - a Database Host, or a MySQL somewhere
            // else entirely. PMA_ARBITRARY turns the login page into one with
            // a server box, so the address is typed at sign-in instead of
            // being baked into the container.
            environment: vec![plain("PMA_ARBITRARY", "1")],
            created_at: epoch,
            is_builtin: true,
        },
    ]
}

/// Whether this id belongs to a template that ships with the app.
pub fn is_builtin_id(id: Uuid) -> bool {
    builtin_templates().iter().any(|template| template.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_has_its_own_stable_id() {
        let ids: std::collections::HashSet<Uuid> = builtin_templates().iter().map(|template| template.id).collect();

        assert_eq!(ids.len(), builtin_templates().len(), "two built-ins share an id");
        // Called twice on purpose: the ids have to be the same list every
        // time, which a `Uuid::new_v4()` slipping in here would break.
        assert_eq!(ids, builtin_templates().iter().map(|template| template.id).collect());
    }

    /// The reason the whole module exists - a built-in that forgot the
    /// variable the image needs would be worse than no template at all,
    /// because it would look like the question had been answered.
    #[test]
    fn the_mariadb_template_carries_the_password_the_image_wont_start_without() {
        let templates = builtin_templates();
        let mariadb = templates.iter().find(|template| template.blueprint_id == "mariadb").expect("a MariaDB built-in");

        let root = mariadb.environment.iter().find(|row| row.key == "MYSQL_ROOT_PASSWORD").expect("MYSQL_ROOT_PASSWORD");
        assert!(root.is_secret, "a database password is a secret row");
        assert!(root.value.is_empty(), "a secret value never travels in a template");
    }

    /// The host is the one value a template cannot know, so shipping a
    /// guess at it would send people to a hostname that does not resolve.
    #[test]
    fn no_phpmyadmin_template_guesses_at_a_host() {
        for template in builtin_templates().iter().filter(|template| template.blueprint_id == "phpmyadmin") {
            assert!(
                !template.environment.iter().any(|row| row.key == "PMA_HOST"),
                "{} ships a PMA_HOST, which only the wizard's picker can know",
                template.name
            );
        }
    }

    /// The frontend translates a built-in's name by matching these exact
    /// strings (`src/i18n/blueprintTranslations.ts`), so they are part of
    /// the contract rather than an implementation detail - a changed id
    /// silently falls back to the English name.
    #[test]
    fn the_ids_are_the_ones_the_frontend_matches_on() {
        let ids: Vec<String> = builtin_templates().iter().map(|template| template.id.to_string()).collect();

        assert_eq!(
            ids,
            vec![
                "7b1d0001-0000-4000-8000-564249424553",
                "7b1d0002-0000-4000-8000-564249424553",
                "7b1d0004-0000-4000-8000-564249424553",
                "7b1d0003-0000-4000-8000-564249424553",
            ]
        );
    }

    #[test]
    fn a_saved_templates_id_is_not_mistaken_for_a_builtin() {
        assert!(!is_builtin_id(Uuid::new_v4()));
        assert!(is_builtin_id(builtin_templates()[0].id));
    }
}
