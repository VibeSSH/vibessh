//! Turning one Pterodactyl server into the VibeSSH Application it should
//! become.
//!
//! **The rule this whole module is built on: never silently change what
//! runs.** VibeSSH's Paper image is not "Paper" in the abstract - it
//! downloads a specific Paper build for a specific Minecraft version. Mapping
//! somebody's 1.19.2 server onto it because the egg was called "Paper" would
//! hand them a different server than the one they had, and they would find
//! out from their players. So a native image is chosen only when the panel
//! tells us the exact version it was running; when it does not, the server
//! becomes a Generic Java Application that keeps running the very jar that
//! was copied across.
//!
//! The same reasoning makes `generic-docker` the last resort rather than a
//! failure: an egg this app has never heard of still has a Docker image and a
//! startup command, and running exactly those reproduces the server without
//! understanding it. Nothing in a panel is left behind for want of a mapping.
//!
//! **Every explanation is a code, not a sentence.** The plan is read by
//! somebody deciding whether to agree to it, in their own language, and a
//! reason assembled here in English would arrive as English in a Polish
//! interface - the one thing this project has already had to fix twice. So
//! each choice carries a `PlanNote` the frontend translates, with the
//! specifics as parameters.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::models::Server;

/// Pterodactyl's own plumbing, injected into every container's environment.
///
/// These describe the panel, not the service: `SERVER_PORT` is whichever
/// allocation the panel handed out, `SERVER_MEMORY` is its limit in MB, and
/// carrying them into VibeSSH would mean an Application whose Environment tab
/// claims things that stopped being true the moment it moved. VibeSSH sets
/// its own equivalents from the Ports tab and the resource limits.
const PANEL_INJECTED: &[&str] = &[
    "STARTUP",
    "SERVER_MEMORY",
    "SERVER_IP",
    "SERVER_PORT",
    "P_SERVER_LOCATION",
    "P_SERVER_UUID",
    "P_SERVER_ALLOCATION_LIMIT",
];

/// Consumed into a blueprint field rather than carried as an environment
/// variable, because that is what it becomes on this side.
const CONSUMED_INTO_FIELDS: &[&str] = &["SERVER_JARFILE", "MINECRAFT_VERSION", "BUILD_NUMBER", "EULA"];

/// One thing the plan has to say, in a form the interface can say in the
/// reader's own language.
///
/// `code` names a translation key; `params` carries the specifics that
/// belong in it. Nothing here is a finished sentence, deliberately.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanNote {
    pub code: &'static str,
    pub params: BTreeMap<String, String>,
}

impl PlanNote {
    pub fn new(code: &'static str) -> Self {
        Self { code, params: BTreeMap::new() }
    }

    pub fn with(code: &'static str, params: &[(&str, &str)]) -> Self {
        Self { code, params: params.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect() }
    }
}

/// Which VibeSSH image a Pterodactyl server becomes, and with what.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePlan {
    pub blueprint_id: &'static str,
    /// Values for that image's own fields, keyed exactly as the blueprint
    /// declares them - this is what the create-from-blueprint path expects.
    pub fields: BTreeMap<String, Value>,
    /// Why this image.
    pub reason: PlanNote,
    /// What the person should decide for themselves before agreeing.
    pub warnings: Vec<PlanNote>,
}

/// The Java major version the panel was running it under, read from the
/// image tag.
///
/// Pterodactyl's own images are `ghcr.io/pterodactyl/yolks:java_17`; people
/// also use `openjdk:17-slim` and `eclipse-temurin:21-jre`. All three carry
/// the number in the tag, and getting it wrong is not cosmetic - a 1.20 world
/// will not open under Java 11, and a plugin built for 21 will not load under
/// 17.
pub fn java_version_from_image(image: &str) -> Option<String> {
    let tag = image.rsplit(':').next()?;
    let digits: String = tag
        .split(|c: char| !c.is_ascii_digit())
        .find(|part| !part.is_empty())
        .unwrap_or_default()
        .to_string();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

/// Whether a version string is a real version rather than a moving target.
///
/// `latest`, an empty value, or an unresolved `{{VARIABLE}}` all mean the
/// panel would have decided at download time. Treating any of them as a
/// version is how somebody ends up on a different Minecraft release than the
/// one their world was saved by.
fn concrete_version(value: Option<&Value>) -> Option<String> {
    let text = match value {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Number(number)) => number.to_string(),
        _ => return None,
    };
    if text.is_empty() || text.contains("{{") {
        return None;
    }
    let looks_like_a_version = text.chars().next().is_some_and(|c| c.is_ascii_digit()) && text.chars().all(|c| c.is_ascii_digit() || c == '.');
    looks_like_a_version.then_some(text)
}

/// The jar the panel was starting, which is also the jar that will be sitting
/// in the copied working directory.
fn jar_path(server: &Server) -> String {
    if let Some(Value::String(jar)) = server.container.environment.get("SERVER_JARFILE") {
        let jar = jar.trim();
        if !jar.is_empty() && !jar.contains("{{") {
            return jar.to_string();
        }
    }
    "server.jar".to_string()
}

/// The egg's name, or the image if the egg was not included.
fn egg_name(server: &Server) -> String {
    server
        .relationships
        .egg
        .as_ref()
        .map(|egg| egg.attributes.name.to_lowercase())
        .unwrap_or_else(|| server.container.image.to_lowercase())
}

fn java_fields(server: &Server, plan: &mut BTreeMap<String, Value>) {
    if let Some(java) = java_version_from_image(&server.container.image) {
        plan.insert("javaVersion".to_string(), Value::String(java));
    }
}

/// A Minecraft server or proxy whose exact version the panel knows, mapped
/// onto the VibeSSH image that manages that same software.
fn native_minecraft(server: &Server, blueprint_id: &'static str, version_field: &str, version: String, eula: bool) -> ImagePlan {
    let mut fields = BTreeMap::new();
    fields.insert(version_field.to_string(), Value::String(version.clone()));
    java_fields(server, &mut fields);
    if eula {
        fields.insert("eulaAccepted".to_string(), Value::Bool(true));
    }

    let mut warnings = Vec::new();
    if eula {
        // Not a decision being made on the operator's behalf: the server was
        // already running under the panel, which Minecraft does not do until
        // the EULA has been accepted. Said out loud all the same, because
        // ticking a licence box silently would be the wrong habit.
        warnings.push(PlanNote::new("eulaCarried"));
    }
    warnings.push(PlanNote::with("jarReplaced", &[("image", blueprint_id), ("version", &version)]));

    ImagePlan { blueprint_id, fields, reason: PlanNote::with("nativeVersion", &[("image", blueprint_id), ("version", &version)]), warnings }
}

/// Whatever the panel was starting, started the same way.
fn generic_java(server: &Server, reason: PlanNote, mut warnings: Vec<PlanNote>) -> ImagePlan {
    let mut fields = BTreeMap::new();
    fields.insert("jarPath".to_string(), Value::String(jar_path(server)));
    java_fields(server, &mut fields);
    if !fields.contains_key("javaVersion") {
        warnings.push(PlanNote::new("noJavaVersion"));
    }
    ImagePlan { blueprint_id: "generic-java", fields, reason, warnings }
}

/// The last resort, and a complete one: the same image, the same command.
fn generic_docker(server: &Server, egg: &str) -> ImagePlan {
    let mut fields = BTreeMap::new();
    fields.insert("image".to_string(), Value::String(server.container.image.clone()));
    // Split on whitespace rather than shipped as one string: the field takes
    // one argument per entry, and the panel's startup command is a plain
    // command line.
    let command: Vec<Value> = server.container.startup_command.split_whitespace().map(|part| json!(part)).collect();
    if !command.is_empty() {
        fields.insert("command".to_string(), Value::Array(command));
    }
    ImagePlan {
        blueprint_id: "generic-docker",
        fields,
        reason: PlanNote::with("noMatchingImage", &[("egg", egg)]),
        warnings: vec![PlanNote::new("genericDocker")],
    }
}

/// The fallback shared by every native Minecraft image whose version the
/// panel would not name.
fn kept_own_jar(server: &Server, image: &'static str) -> ImagePlan {
    generic_java(
        server,
        PlanNote::with("noConcreteVersion", &[("image", image)]),
        vec![PlanNote::with("keptOwnJar", &[("image", image)])],
    )
}

/// Decides what one Pterodactyl server becomes.
pub fn map_server(server: &Server) -> ImagePlan {
    let egg = egg_name(server);
    let environment = &server.container.environment;
    let minecraft_version = concrete_version(environment.get("MINECRAFT_VERSION"));
    let proxy_version = || concrete_version(environment.get("MINECRAFT_VERSION")).or_else(|| concrete_version(environment.get("BUILD_NUMBER")));

    // Order matters: eggs are named freely ("Purpur (a Paper fork)"), and the
    // more specific fork has to be tested first.
    if egg.contains("purpur") {
        return match minecraft_version {
            Some(version) => native_minecraft(server, "purpur", "purpurVersion", version, true),
            None => kept_own_jar(server, "purpur"),
        };
    }
    if egg.contains("paper") {
        return match minecraft_version {
            Some(version) => native_minecraft(server, "paper", "minecraftVersion", version, true),
            None => kept_own_jar(server, "paper"),
        };
    }
    if egg.contains("velocity") {
        return match proxy_version() {
            Some(version) => native_minecraft(server, "velocity", "velocityVersion", version, false),
            None => kept_own_jar(server, "velocity"),
        };
    }
    if egg.contains("waterfall") {
        return match proxy_version() {
            Some(version) => native_minecraft(server, "waterfall", "waterfallVersion", version, false),
            None => kept_own_jar(server, "waterfall"),
        };
    }
    if egg.contains("bungeecord") {
        // Deliberately *not* mapped onto Waterfall. Waterfall is a fork of
        // BungeeCord, not the same program, and swapping one for the other is
        // exactly the silent substitution this module exists to refuse.
        return generic_java(server, PlanNote::new("bungeecordEgg"), vec![PlanNote::new("bungeecordNotWaterfall")]);
    }

    // Every other Java server: Vanilla, Spigot, Forge, Fabric, Sponge, the
    // modpack eggs. They differ enormously from each other and not at all in
    // what matters here - a jar, started by a JVM, in a directory that is
    // being copied across whole.
    for java_egg in ["vanilla", "spigot", "bukkit", "forge", "fabric", "sponge", "mohist", "magma", "craftbukkit", "minecraft"] {
        if egg.contains(java_egg) {
            return generic_java(server, PlanNote::with("javaEgg", &[("egg", java_egg)]), Vec::new());
        }
    }

    if egg.contains("redis") {
        let mut fields = BTreeMap::new();
        if let Some(version) = java_version_from_image(&server.container.image) {
            fields.insert("redisVersion".to_string(), Value::String(version));
        }
        return ImagePlan {
            blueprint_id: "redis",
            fields,
            reason: PlanNote::new("redisEgg"),
            warnings: vec![PlanNote::new("checkRedisVersion")],
        };
    }
    if egg.contains("mariadb") || egg.contains("mysql") {
        return ImagePlan {
            blueprint_id: "mariadb",
            fields: BTreeMap::new(),
            reason: PlanNote::new("mariadbEgg"),
            warnings: vec![PlanNote::new("setMysqlRootPassword")],
        };
    }
    if egg.contains("node") || egg.contains("javascript") || egg.contains("discord.js") {
        let mut fields = BTreeMap::new();
        if let Some(Value::String(entry)) = environment.get("MAIN_FILE").or_else(|| environment.get("JS_FILE")) {
            fields.insert("entryFile".to_string(), Value::String(entry.clone()));
        }
        return ImagePlan {
            blueprint_id: "nodejs-bot",
            fields,
            reason: PlanNote::new("nodeEgg"),
            warnings: vec![PlanNote::new("checkEntryFile")],
        };
    }
    if egg.contains("python") || egg.contains("discord.py") {
        let mut fields = BTreeMap::new();
        if let Some(Value::String(entry)) = environment.get("PY_FILE").or_else(|| environment.get("MAIN_FILE")) {
            fields.insert("entryFile".to_string(), Value::String(entry.clone()));
        }
        return ImagePlan {
            blueprint_id: "python-bot",
            fields,
            reason: PlanNote::new("pythonEgg"),
            warnings: vec![PlanNote::new("checkEntryFile")],
        };
    }

    generic_docker(server, &egg)
}

/// The environment variables worth carrying over.
///
/// Everything the panel injected about itself is dropped, and everything
/// consumed into a blueprint field is dropped too - a `SERVER_JARFILE` that
/// became `jarPath` would otherwise sit in the Environment tab looking
/// authoritative while changing nothing.
pub fn environment_to_carry(server: &Server) -> Vec<(String, String)> {
    server
        .container
        .environment
        .iter()
        .filter(|(key, _)| !PANEL_INJECTED.contains(&key.as_str()) && !CONSUMED_INTO_FIELDS.contains(&key.as_str()))
        .map(|(key, value)| {
            let rendered = match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            (key.clone(), rendered)
        })
        .collect()
}

/// Pterodactyl's limits in VibeSSH's units.
///
/// `0` means unlimited in Pterodactyl. Copying it across as a limit of zero
/// would produce an Application that cannot start, which is the worst
/// possible reading of "no limit".
pub fn resource_limits(server: &Server) -> (Option<i64>, Option<f64>) {
    let memory_mb = (server.limits.memory > 0).then_some(server.limits.memory);
    // Pterodactyl counts CPU in percent of a single core: 200 is two cores.
    let cpu_cores = (server.limits.cpu > 0).then(|| server.limits.cpu as f64 / 100.0);
    (memory_mb, cpu_cores)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pterodactyl::models::{Container, Egg, ServerRelationships, Wrapped};

    fn server(egg: &str, image: &str, environment: &[(&str, Value)]) -> Server {
        Server {
            id: 1,
            uuid: "1a7ce997-259b-452e-8b4e-cecc464142ca".to_string(),
            identifier: "1a7ce997".to_string(),
            name: "Survival".to_string(),
            description: None,
            suspended: false,
            limits: Default::default(),
            node: 1,
            egg: 5,
            container: Container {
                startup_command: "java -jar server.jar".to_string(),
                image: image.to_string(),
                environment: environment.iter().map(|(key, value)| (key.to_string(), value.clone())).collect(),
            },
            relationships: ServerRelationships {
                allocations: None,
                egg: Some(Wrapped { attributes: Egg { id: 5, name: egg.to_string(), description: None } }),
            },
        }
    }

    fn codes(notes: &[PlanNote]) -> Vec<&'static str> {
        notes.iter().map(|note| note.code).collect()
    }

    #[test]
    fn a_paper_server_with_a_known_version_becomes_the_paper_image() {
        let plan = map_server(&server("Paper", "ghcr.io/pterodactyl/yolks:java_17", &[("MINECRAFT_VERSION", json!("1.20.1"))]));
        assert_eq!(plan.blueprint_id, "paper");
        assert_eq!(plan.fields.get("minecraftVersion"), Some(&json!("1.20.1")));
        assert_eq!(plan.fields.get("javaVersion"), Some(&json!("17")), "the Java version must survive the move");
        assert_eq!(plan.fields.get("eulaAccepted"), Some(&json!(true)));
        assert_eq!(plan.reason.code, "nativeVersion");
        assert_eq!(plan.reason.params.get("version"), Some(&"1.20.1".to_string()));
    }

    /// The rule the whole module is built on. "latest" is not a version, and
    /// resolving it here would hand somebody a different server than they
    /// had.
    #[test]
    fn a_paper_server_on_latest_keeps_its_own_jar_instead() {
        let plan = map_server(&server("Paper", "ghcr.io/pterodactyl/yolks:java_17", &[("MINECRAFT_VERSION", json!("latest"))]));
        assert_eq!(plan.blueprint_id, "generic-java");
        assert_eq!(plan.fields.get("jarPath"), Some(&json!("server.jar")));
        assert_eq!(plan.reason.code, "noConcreteVersion");
        assert!(codes(&plan.warnings).contains(&"keptOwnJar"), "a decision like this has to be visible");
    }

    #[test]
    fn an_unresolved_panel_variable_is_not_a_version_either() {
        let plan = map_server(&server("Paper", "yolks:java_21", &[("MINECRAFT_VERSION", json!("{{MC_VERSION}}"))]));
        assert_eq!(plan.blueprint_id, "generic-java");
    }

    /// Waterfall is a fork of BungeeCord, not the same program. Substituting
    /// one for the other is the exact failure this module refuses.
    #[test]
    fn bungeecord_is_not_quietly_replaced_with_waterfall() {
        let plan = map_server(&server("BungeeCord", "yolks:java_17", &[]));
        assert_eq!(plan.blueprint_id, "generic-java");
        assert!(codes(&plan.warnings).contains(&"bungeecordNotWaterfall"));
    }

    #[test]
    fn purpur_is_matched_before_paper() {
        let plan = map_server(&server("Purpur (a Paper fork)", "yolks:java_21", &[("MINECRAFT_VERSION", json!("1.21"))]));
        assert_eq!(plan.blueprint_id, "purpur");
        assert_eq!(plan.fields.get("purpurVersion"), Some(&json!("1.21")));
    }

    /// Nothing in a panel may be left behind for want of a mapping.
    #[test]
    fn an_unknown_egg_still_migrates_as_its_own_image_and_command() {
        let mut source = server("Rust Server", "docker.io/some/rust:latest", &[]);
        source.container.startup_command = "./RustDedicated -batchmode +server.port 28015".to_string();
        let plan = map_server(&source);
        assert_eq!(plan.blueprint_id, "generic-docker");
        assert_eq!(plan.fields.get("image"), Some(&json!("docker.io/some/rust:latest")));
        assert_eq!(plan.fields.get("command").and_then(|c| c.as_array()).map(Vec::len), Some(4));
        assert_eq!(plan.reason.params.get("egg"), Some(&"rust server".to_string()));
    }

    #[test]
    fn the_jar_the_panel_named_is_the_jar_that_is_started() {
        let plan = map_server(&server("Forge", "yolks:java_8", &[("SERVER_JARFILE", json!("forge-1.12.2.jar"))]));
        assert_eq!(plan.blueprint_id, "generic-java");
        assert_eq!(plan.fields.get("jarPath"), Some(&json!("forge-1.12.2.jar")));
        assert_eq!(plan.fields.get("javaVersion"), Some(&json!("8")));
    }

    #[test]
    fn java_versions_are_read_from_every_tag_style_in_the_wild() {
        assert_eq!(java_version_from_image("ghcr.io/pterodactyl/yolks:java_17"), Some("17".to_string()));
        assert_eq!(java_version_from_image("openjdk:8-slim"), Some("8".to_string()));
        assert_eq!(java_version_from_image("eclipse-temurin:21-jre"), Some("21".to_string()));
        assert_eq!(java_version_from_image("some/image:latest"), None, "a tag with no number tells us nothing");
    }

    /// The panel's own plumbing describes the panel. Carried across, it would
    /// be an Environment tab full of statements that stopped being true.
    #[test]
    fn the_panels_own_variables_are_not_carried_over() {
        let source = server(
            "Paper",
            "yolks:java_17",
            &[
                ("SERVER_PORT", json!("25565")),
                ("SERVER_MEMORY", json!("4096")),
                ("SERVER_JARFILE", json!("server.jar")),
                ("MY_PLUGIN_TOKEN", json!("keep-me")),
            ],
        );
        let carried = environment_to_carry(&source);
        assert_eq!(carried, vec![("MY_PLUGIN_TOKEN".to_string(), "keep-me".to_string())]);
    }

    /// Zero means unlimited in Pterodactyl. Copied across literally it would
    /// be an Application that cannot start.
    #[test]
    fn unlimited_in_the_panel_does_not_become_a_limit_of_zero() {
        let mut source = server("Paper", "yolks:java_17", &[]);
        source.limits.memory = 0;
        source.limits.cpu = 0;
        assert_eq!(resource_limits(&source), (None, None));

        source.limits.memory = 4096;
        source.limits.cpu = 200;
        assert_eq!(resource_limits(&source), (Some(4096), Some(2.0)));
    }

    /// Nothing in a plan may reach the interface as a finished English
    /// sentence: it is read by somebody deciding, in their own language.
    #[test]
    fn no_explanation_is_a_sentence() {
        for egg in ["Paper", "BungeeCord", "Rust Server", "Redis", "MariaDB", "Node.js", "Python", "Forge", "Velocity"] {
            let plan = map_server(&server(egg, "yolks:java_17", &[]));
            let mut all = vec![plan.reason];
            all.extend(plan.warnings);
            for note in all {
                assert!(!note.code.contains(' '), "{egg}: {:?} looks like prose, not a key", note.code);
                assert!(!note.code.is_empty(), "{egg}: an explanation with no key cannot be translated");
            }
        }
    }
}
