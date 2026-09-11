use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::RuntimeType;

/// A saved starting point for the Create Application wizard.
///
/// **What it is not.** Not a link to the Applications made from it: changing
/// a template later leaves them alone, and deleting one leaves them running.
/// It is the set of answers the wizard would otherwise have to be given
/// again, and nothing more - which is also why it carries no `server_id`.
/// Where an Application runs is the one thing that genuinely differs each
/// time, and prefilling it would put the same server in front of somebody who
/// opened the template precisely to use a different one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationTemplate {
    pub id: Uuid,
    /// What the person called it, e.g. "Paper 1.21 z pluginami".
    pub name: String,
    pub blueprint_id: String,
    pub runtime_type: RuntimeType,
    /// The blueprint's own fields, keyed by `BlueprintField::key` - the same
    /// shape the wizard collects and `render_runtime_config` consumes.
    pub field_values: serde_json::Value,
    pub environment: Vec<TemplateEnvironmentVariable>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Shipped with the app rather than saved by the user - see
    /// `storage::builtin_templates`. Read-only: it cannot be edited or
    /// deleted, and the frontend leaves the delete button off it.
    ///
    /// Defaulted rather than required so a templates file written before
    /// built-ins existed still parses, with every template in it correctly
    /// reading as the user's own.
    #[serde(default)]
    pub is_builtin: bool,
}

/// One environment row in a template.
///
/// **A secret's value is never in here.** Secret values live in the OS
/// credential store and are not even handed to the frontend for an existing
/// Application, so a template built from the wizard would be the one place in
/// the app where one sat in a plain file - which is exactly what
/// `storage::ai_config` refuses to do with the API key, for the same reasons:
/// a config file is readable by anything running as this user, ends up in
/// backups, and turns up in screenshots.
///
/// So a secret row travels as its name and nothing else, and the wizard asks
/// for the value each time the template is used. That is not an oversight to
/// be tidied up later; it is the feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateEnvironmentVariable {
    pub key: String,
    /// Empty whenever `is_secret` is true, enforced on the way in rather
    /// than trusted from the caller.
    pub value: String,
    pub is_secret: bool,
}
