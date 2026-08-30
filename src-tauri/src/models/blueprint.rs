//! The Blueprint domain model - see `blueprints` module's own doc comment
//! for how this data connects to the four `ApplicationRuntime`
//! implementations, and docs/APPLICATIONS_ARCHITECTURE.md Section 7.5 for
//! why `features` here is a different concept from host-level
//! `AgentCapabilities`.

use serde::{Deserialize, Serialize};

use super::RuntimeType;

/// A declarative description of "what kind of application is this" - which
/// `RuntimeType`s it can run under, which UI features it needs
/// (`features`), and which inputs a Create Application wizard would ask
/// for (`fields`). Behavior (turning filled-in `fields` into a concrete
/// `runtime_config`) lives separately, in a `blueprints::BlueprintHandler`
/// - this struct is pure data, serializable as-is to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blueprint {
    /// e.g. `"generic"`, `"generic-java"` - stored verbatim as
    /// `Application::blueprint_id`.
    pub id: String,
    pub name: String,
    pub description: String,
    /// The version of *this struct's own shape* - bumped if
    /// `Blueprint`/`BlueprintField`'s fields themselves ever change in a
    /// way older stored definitions couldn't parse. Distinct from
    /// `blueprint_version`, which versions one specific blueprint's own
    /// content.
    pub schema_version: i32,
    pub blueprint_version: i32,
    pub supported_runtime_types: Vec<RuntimeType>,
    pub features: Vec<BlueprintFeature>,
    pub fields: Vec<BlueprintField>,
    pub is_builtin: bool,
}

/// UI-facing capabilities this application exposes - not host-level
/// `AgentCapabilities` (does the host have Docker/systemd at all), a
/// different concept covering what tabs/actions the Application detail
/// page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlueprintFeature {
    Console,
    Logs,
    Environment,
    Ports,
}

/// One input a Create Application wizard would collect for this blueprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintField {
    pub key: String,
    pub label: String,
    pub field_type: BlueprintFieldType,
    pub required: bool,
    /// JSON rather than a typed value, matching `field_type` - so a
    /// `TextList` field's default can be a real `[]`/`["a","b"]`, not a
    /// stringly-typed encoding of one.
    pub default_value: Option<serde_json::Value>,
    pub help_text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlueprintFieldType {
    Text,
    Path,
    Number,
    Boolean,
    TextList,
    /// A path, same as `Path` for validation/storage purposes - the
    /// frontend renders it differently: a picker populated from
    /// `detect_java_installations` (real, actually-installed JVMs) with a
    /// free-text fallback, rather than a plain input the user has to know
    /// a path for. Not a generic "Select" type - there's nothing else
    /// today that needs host-detected, dynamically-populated options, and
    /// inventing that generality before a second user exists would be
    /// speculative.
    JavaVersion,
    /// A string, same as `Text` for validation/storage purposes - the
    /// frontend renders it as a picker populated from whichever PaperMC
    /// project's release list matches the field (`list_paper_versions` /
    /// `list_velocity_versions`, keyed off the field's own `key`), same
    /// "dynamic picker, not a hand-typed value" shape as `JavaVersion`.
    /// Named for the API family, not "Minecraft version" - Velocity's own
    /// release numbers (`"3.4.0"`) aren't Minecraft version numbers at all,
    /// even though Paper's happen to be.
    PapermcVersion,
}
