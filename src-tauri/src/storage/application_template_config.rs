//! Saved wizard answers, as a plain JSON file in the app config dir - the
//! same load-or-create shape `ai_config`, `cloud_config` and
//! `backup_destination_config` already use.
//!
//! No secret value is ever in this file. A template's secret rows carry their
//! names only, and `save_template` empties the value rather than trusting the
//! caller not to send one - see `TemplateEnvironmentVariable` for why.

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::ApplicationTemplate;

const CONFIG_FILE_NAME: &str = "application_templates.json";

/// More than anybody will make by hand, and low enough that a corrupted or
/// hand-edited file cannot grow without bound.
const MAX_TEMPLATES: usize = 200;

fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_FILE_NAME)
}

/// A missing file means none have been saved, which is the normal state on a
/// fresh install rather than an error.
///
/// The user's own only - `all_templates` is what the UI asks for.
pub fn load_templates(config_dir: &Path) -> AppResult<Vec<ApplicationTemplate>> {
    let path = config_path(config_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(&path).map_err(|err| AppError::Storage(format!("failed to read {CONFIG_FILE_NAME}: {err}")))?;
    serde_json::from_slice(&bytes).map_err(|err| AppError::Storage(format!("{CONFIG_FILE_NAME} is not readable: {err}")))
}

/// What the wizard lists: the ones that ship with VibeSSH, then the user's
/// own.
///
/// Built-ins first because they are the answer to "I have never done this
/// before", which is who is reading the list at all - somebody with their own
/// saved templates knows where to find them.
pub fn all_templates(config_dir: &Path) -> AppResult<Vec<ApplicationTemplate>> {
    let mut templates = super::builtin_templates::builtin_templates();
    templates.extend(load_templates(config_dir)?);
    Ok(templates)
}

fn write_templates(config_dir: &Path, templates: &[ApplicationTemplate]) -> AppResult<()> {
    std::fs::create_dir_all(config_dir).map_err(|err| AppError::Storage(format!("failed to create the config directory: {err}")))?;
    let json = serde_json::to_vec_pretty(templates).map_err(|err| AppError::Storage(format!("failed to serialize templates: {err}")))?;
    std::fs::write(config_path(config_dir), json).map_err(|err| AppError::Storage(format!("failed to write {CONFIG_FILE_NAME}: {err}")))
}

/// Adds a template, or replaces the one with the same id.
///
/// Secret values are dropped here rather than at the caller: this is the last
/// place before the value would reach the disk, so it is the place where "a
/// secret is never written to a config file" can actually be guaranteed.
pub fn save_template(config_dir: &Path, mut template: ApplicationTemplate) -> AppResult<ApplicationTemplate> {
    let name = template.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::InvalidInput("a template needs a name".into()));
    }
    template.name = name;
    // A built-in is what `storage::builtin_templates` says it is. Letting one
    // be saved over would put a copy in this file that the list then has to
    // choose between, and the copy would keep whatever it was edited into
    // long after the shipped one changed.
    if super::builtin_templates::is_builtin_id(template.id) {
        return Err(AppError::InvalidInput("that template ships with VibeSSH - save it under a new name instead".into()));
    }
    // Never stored as one either: `is_builtin` is a fact about where a
    // template came from, and this one came from the user.
    template.is_builtin = false;

    for variable in &mut template.environment {
        if variable.is_secret {
            variable.value.clear();
        }
    }

    let mut templates = load_templates(config_dir)?;
    match templates.iter_mut().find(|existing| existing.id == template.id) {
        Some(existing) => *existing = template.clone(),
        None => {
            if templates.len() >= MAX_TEMPLATES {
                return Err(AppError::InvalidInput(format!("there is room for {MAX_TEMPLATES} templates - delete one first")));
            }
            templates.push(template.clone());
        }
    }

    write_templates(config_dir, &templates)?;
    Ok(template)
}

/// Removing one that is already gone is not an error - the caller wanted it
/// absent, and it is.
pub fn delete_template(config_dir: &Path, id: Uuid) -> AppResult<()> {
    if super::builtin_templates::is_builtin_id(id) {
        return Err(AppError::InvalidInput("that template ships with VibeSSH and can't be deleted".into()));
    }
    let mut templates = load_templates(config_dir)?;
    templates.retain(|template| template.id != id);
    write_templates(config_dir, &templates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{RuntimeType, TemplateEnvironmentVariable};

    /// Same shape `ai_config`'s own tests use - a directory that does not
    /// exist yet, so the load-or-create path is exercised rather than
    /// assumed.
    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("vibessh-templates-test-{}", Uuid::new_v4()))
    }

    fn template(name: &str) -> ApplicationTemplate {
        ApplicationTemplate {
            id: Uuid::new_v4(),
            name: name.to_string(),
            blueprint_id: "paper".to_string(),
            runtime_type: RuntimeType::Docker,
            field_values: serde_json::json!({ "minecraftVersion": "1.21.1" }),
            environment: vec![],
            created_at: chrono::Utc::now(),
            is_builtin: false,
        }
    }

    #[test]
    fn a_fresh_install_has_none_rather_than_failing() {
        let dir = temp_dir();

        assert!(load_templates(&dir).unwrap().is_empty());
    }

    #[test]
    fn saving_then_loading_returns_what_was_saved() {
        let dir = temp_dir();
        let saved = save_template(&dir, template("Paper")).unwrap();

        let loaded = load_templates(&dir).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, saved.id);
        assert_eq!(loaded[0].field_values["minecraftVersion"], "1.21.1");
    }

    /// The reason this module exists in this shape. A template is a plain
    /// file, so a secret value reaching it would be a secret in plaintext -
    /// and the caller is not trusted to have stripped it.
    #[test]
    fn a_secret_value_never_reaches_the_file() {
        let dir = temp_dir();
        let mut with_secret = template("Paper");
        with_secret.environment = vec![
            TemplateEnvironmentVariable { key: "RCON_PASSWORD".into(), value: "hunter2".into(), is_secret: true },
            TemplateEnvironmentVariable { key: "EULA".into(), value: "true".into(), is_secret: false },
        ];

        save_template(&dir, with_secret).unwrap();

        let raw = std::fs::read_to_string(dir.join(CONFIG_FILE_NAME)).unwrap();
        assert!(!raw.contains("hunter2"), "the secret was written to disk: {raw}");
        // The name survives, so the wizard can ask for the value again.
        assert!(raw.contains("RCON_PASSWORD"), "{raw}");
        // A non-secret value is the whole point of a template, and stays.
        assert!(raw.contains("true"), "{raw}");
    }

    #[test]
    fn saving_the_same_id_replaces_rather_than_duplicates() {
        let dir = temp_dir();
        let first = save_template(&dir, template("Paper")).unwrap();
        let mut renamed = first.clone();
        renamed.name = "Paper z pluginami".to_string();

        save_template(&dir, renamed).unwrap();

        let loaded = load_templates(&dir).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Paper z pluginami");
    }

    #[test]
    fn a_nameless_template_is_refused() {
        let dir = temp_dir();
        let mut nameless = template("   ");

        nameless.name = "   ".to_string();

        assert!(save_template(&dir, nameless).is_err());
    }

    #[test]
    fn deleting_one_that_is_already_gone_is_fine() {
        let dir = temp_dir();
        save_template(&dir, template("Paper")).unwrap();

        delete_template(&dir, Uuid::new_v4()).unwrap();

        assert_eq!(load_templates(&dir).unwrap().len(), 1);
    }
}
