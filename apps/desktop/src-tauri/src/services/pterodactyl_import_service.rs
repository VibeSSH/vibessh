//! Building the plan for a migration from Pterodactyl, and nothing else.
//!
//! **Why a plan is a separate thing from an import.** Everything this
//! produces is a proposal: which VibeSSH image each server becomes, which
//! ports it keeps, which of its environment variables survive, where its
//! files are and which Node they are on. A person reads it and agrees to it
//! before a single Application is created. That is the same stance the
//! assistant's context preview takes (`AGENTS.md` §6 - a boundary the
//! interface does not show is not a boundary), and it matters more here,
//! because the alternative is finding out what the importer decided by
//! looking at what it already did.
//!
//! The plan is also where the honest gaps are recorded rather than papered
//! over: a Pterodactyl node with no matching VibeSSH Node, a database whose
//! password the panel will not reveal, an egg with no equivalent. Each one
//! becomes a note on the thing it affects, so the person agreeing knows what
//! they are agreeing to.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::errors::AppResult;
use crate::pterodactyl::mapping::{environment_to_carry, resource_limits, PlanNote};
use crate::pterodactyl::models::{Node, Server};
use crate::pterodactyl::{map_server, PterodactylClient};
use crate::storage::application_repository::ApplicationRepository;
use crate::storage::server_repository::ServerRepository;

/// Where Pterodactyl keeps server volumes when a node does not say
/// otherwise. Read from the node's own `daemon_base` first - an operator who
/// moved it onto a bigger disk is exactly the person whose files must still
/// be found.
const DEFAULT_DAEMON_BASE: &str = "/var/lib/pterodactyl/volumes";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPlan {
    pub panel_url: String,
    /// How many servers the panel reports, which is not always how many are
    /// in `servers` - a suspended one is listed separately below.
    pub total_servers: u32,
    pub servers: Vec<PlannedServer>,
    /// Anything that applies to the whole migration rather than to one
    /// server. Keys, like every other explanation here.
    pub notes: Vec<PlanNote>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedServer {
    pub source_id: i64,
    /// The volume directory name on the daemon - this is how the files are
    /// found.
    pub source_uuid: String,
    pub name: String,
    pub egg: String,
    pub suspended: bool,

    pub blueprint_id: String,
    /// Why that image was chosen. A key the interface translates, never a
    /// finished sentence - see `pterodactyl::mapping`'s own doc comment.
    pub reason: PlanNote,
    pub fields: BTreeMap<String, Value>,

    pub ports: Vec<PlannedPort>,
    pub environment: Vec<PlannedEnvironmentVariable>,
    pub memory_mb: Option<i64>,
    pub cpu_cores: Option<f64>,
    pub databases: Vec<PlannedDatabase>,

    pub source: PlannedSource,
    /// What the person should decide before agreeing to this one.
    pub warnings: Vec<PlanNote>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedPort {
    pub port: u16,
    /// Pterodactyl's own "primary" allocation - the one players connect to,
    /// and the one worth keeping if any port has to change.
    pub primary: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedEnvironmentVariable {
    pub key: String,
    pub value: String,
}

/// One database Pterodactyl made for this server.
///
/// `password_available` is always false, and saying so on every row is the
/// point: the Application API does not return database passwords, so the
/// data has to be dumped from the database server itself rather than by
/// connecting as this user. A person planning a migration needs to know that
/// before they start, not when it fails.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedDatabase {
    pub name: String,
    pub username: String,
    pub password_available: bool,
    /// The address of the machine the panel keeps this database on.
    ///
    /// Read from the panel rather than assumed to be the node the server's
    /// files are on: those are often the same machine and often not, and
    /// taking a dump on the wrong one fails in a way nobody can interpret.
    pub host_address: String,
    /// The VibeSSH Node that is that machine, when one is. Without it there
    /// is no way to reach the data at all, because the panel will not reveal
    /// the password that would let anything connect over the network.
    pub host_server_id: Option<uuid::Uuid>,
    pub host_server_name: Option<String>,
}

/// Which machine this server's files are on, and whether VibeSSH already
/// knows it.
///
/// This is the question that decides how the files move. When the panel's
/// node is a Node VibeSSH already manages, the copy is local to that machine.
/// When it is not, VibeSSH has no way in and the person has to add it as a
/// Node first - which the plan says rather than discovering mid-import.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedSource {
    pub node_name: String,
    pub fqdn: String,
    /// Absolute path to this server's volume on that machine.
    pub volume_path: String,
    /// The VibeSSH Node that is the same machine, when one is.
    pub matched_server_id: Option<uuid::Uuid>,
    pub matched_server_name: Option<String>,
}

/// One "this Pterodactyl node is that VibeSSH Node" answer from the operator.
///
/// Keyed by the panel's fqdn rather than its numeric node id, because the
/// fqdn is what the plan shows and what a person recognises. Ids would be
/// smaller and completely opaque in the interface.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeOverride {
    pub fqdn: String,
    pub server_id: uuid::Uuid,
}

/// The three things about a VibeSSH Node that matching needs. Projected out
/// of the model so the rule below can be tested on its own, without building
/// a whole `Server` row to check a string comparison.
struct KnownNode {
    id: uuid::Uuid,
    name: String,
    host: String,
}

/// Matches a Pterodactyl node to a VibeSSH Node by address.
///
/// Deliberately an exact, case-insensitive match on the host rather than a
/// DNS lookup: resolving names here would make the plan depend on what this
/// desktop machine's resolver happens to say, and a wrong match copies files
/// out of the wrong server. No match is a fine answer - the person picks the
/// Node themselves.
fn match_node<'a>(node: &Node, servers: &'a [KnownNode], overrides: &[NodeOverride]) -> Option<&'a KnownNode> {
    let fqdn = node.fqdn.trim().to_lowercase();
    if fqdn.is_empty() {
        return None;
    }
    // An answer from the operator beats an address comparison every time:
    // they know which machine it is, and the comparison is only a guess that
    // happens to be cheap.
    if let Some(chosen) = overrides.iter().find(|entry| entry.fqdn.trim().to_lowercase() == fqdn) {
        if let Some(known) = servers.iter().find(|server| server.id == chosen.server_id) {
            return Some(known);
        }
    }
    servers.iter().find(|server| server.host.trim().to_lowercase() == fqdn)
}

/// Whether a database host address means "the same machine as whatever is
/// asking".
///
/// A single-box panel routinely records its database host as `127.0.0.1`,
/// which can never equal any Node's own address - so without this every
/// database on such a panel reports an unknown machine, which is both wrong
/// and the most common case there is.
fn is_loopback(address: &str) -> bool {
    let address = address.trim().to_lowercase();
    address == "localhost" || address == "127.0.0.1" || address == "::1" || address.starts_with("127.")
}

fn volume_path(node: &Node, server_uuid: &str) -> String {
    let base = node
        .daemon_base
        .as_deref()
        .map(str::trim)
        .filter(|base| !base.is_empty())
        .unwrap_or(DEFAULT_DAEMON_BASE)
        .trim_end_matches('/');
    format!("{base}/{server_uuid}")
}

fn plan_ports(server: &Server) -> Vec<PlannedPort> {
    let Some(allocations) = server.relationships.allocations.as_ref() else {
        return Vec::new();
    };
    let mut ports: Vec<PlannedPort> = allocations
        .data
        .iter()
        .map(|wrapped| &wrapped.attributes)
        .map(|allocation| PlannedPort { port: allocation.port, primary: allocation.assigned, notes: allocation.notes.clone() })
        .collect();
    // The primary first, because it is the one a person checks and the one
    // that must survive if any port has to be changed.
    ports.sort_by_key(|port| (!port.primary, port.port));
    ports
}

/// Reads the whole panel and works out what it would become here.
///
/// Databases are fetched per server, which is one request each. That is the
/// only shape the Application API offers, and it happens once, in a step the
/// person is waiting on deliberately.
pub async fn build_plan(
    client: &PterodactylClient,
    server_repo: &ServerRepository,
    app_repo: &ApplicationRepository,
    overrides: &[NodeOverride],
) -> AppResult<MigrationPlan> {
    let total_servers = client.check_access().await?;
    // The servers are the plan, so a failure here is fatal. The nodes only
    // say which machine each one is on, and a key without the Nodes
    // permission - or a panel that answers oddly for one collection - must
    // not cost somebody the whole plan. It degrades to "the files could not
    // be located", which is already a case each server card handles.
    let sources = client.list_servers().await?;
    let (nodes, node_note) = match client.list_nodes().await {
        Ok(nodes) => (nodes, None),
        Err(err) => (Vec::new(), Some(PlanNote::with("nodesUnreadable", &[("error", &err.to_string())]))),
    };
    let known: Vec<KnownNode> = server_repo
        .list()?
        .into_iter()
        .map(|server| KnownNode { id: server.id, name: server.name, host: server.host })
        .collect();

    // What is already here, so a second run can say so instead of quietly
    // duplicating it. Read once, not per server.
    let existing: Vec<(String, Option<uuid::Uuid>)> = app_repo
        .list()
        .unwrap_or_default()
        .into_iter()
        .map(|application| (application.name.trim().to_lowercase(), application.server_id))
        .collect();

    let mut notes = Vec::new();
    notes.extend(node_note);
    let mut planned = Vec::with_capacity(sources.len());

    for source in &sources {
        let image = map_server(source);
        let node = nodes.iter().find(|node| node.id == source.node);

        let mut warnings = image.warnings;
        if source.suspended {
            warnings.push(PlanNote::new("suspended"));
        }

        let source_location = match node {
            Some(node) => {
                let matched = match_node(node, &known, overrides);
                if matched.is_none() {
                    warnings.push(PlanNote::with("nodeUnknown", &[("fqdn", &node.fqdn)]));
                }
                PlannedSource {
                    node_name: node.name.clone(),
                    fqdn: node.fqdn.clone(),
                    volume_path: volume_path(node, &source.uuid),
                    matched_server_id: matched.map(|known| known.id),
                    matched_server_name: matched.map(|known| known.name.clone()),
                }
            }
            None => {
                // The panel listed a server on a node it did not list. Not
                // fatal for the plan, but the files cannot be located, so it
                // is said plainly against the server it affects.
                warnings.push(PlanNote::new("nodeMissing"));
                PlannedSource {
                    node_name: format!("node {}", source.node),
                    fqdn: String::new(),
                    volume_path: String::new(),
                    matched_server_id: None,
                    matched_server_name: None,
                }
            }
        };

        let databases = match client.list_server_databases(source.id).await {
            Ok(databases) => databases
                .into_iter()
                .map(|database| {
                    // The address rides in the relationship, which is only
                    // there because the request asked for it. Absent means
                    // "not told", and the plan says so rather than guessing.
                    let address = database.relationships.host.as_ref().map(|host| host.attributes.host.clone()).unwrap_or_default();
                    // A loopback address means the machine the server itself
                    // runs on, so it resolves to whatever that node resolved
                    // to - including an override the operator just gave.
                    // Otherwise the same exact-address rule the file side
                    // uses: a near-match would mean dumping somebody else's
                    // database.
                    let matched = if is_loopback(&address) {
                        node.and_then(|node| match_node(node, &known, overrides))
                    } else {
                        known.iter().find(|node| !address.is_empty() && node.host.trim().to_lowercase() == address.trim().to_lowercase())
                    };
                    PlannedDatabase {
                        name: database.database,
                        username: database.username,
                        password_available: false,
                        host_address: address,
                        host_server_id: matched.map(|node| node.id),
                        host_server_name: matched.map(|node| node.name.clone()),
                    }
                })
                .collect(),
            Err(err) => {
                // One server's databases failing must not lose the whole
                // plan - the rest of the panel is still migratable, and the
                // gap is named.
                warnings.push(PlanNote::with("databaseListFailed", &[("error", &err.to_string())]));
                Vec::new()
            }
        };
        if !databases.is_empty() {
            warnings.push(PlanNote::new("databasePasswords"));
            for database in databases.iter().filter(|database| database.host_server_id.is_none()) {
                // Named per database rather than once, because a server can
                // have databases on two different hosts and only one of them
                // be reachable.
                warnings.push(PlanNote::with(
                    "databaseHostUnknown",
                    &[("database", &database.name), ("host", if database.host_address.is_empty() { "?" } else { &database.host_address })],
                ));
            }
        }

        // Matched on name *and* Node: the same name on a different machine is
        // a different Application, and this is exactly the case somebody
        // creates deliberately when moving a server between Nodes.
        let already = existing
            .iter()
            .any(|(name, server_id)| name == &source.name.trim().to_lowercase() && *server_id == source_location.matched_server_id);
        if already {
            warnings.push(PlanNote::with("alreadyImported", &[("name", &source.name)]));
        }

        let (memory_mb, cpu_cores) = resource_limits(source);
        planned.push(PlannedServer {
            source_id: source.id,
            source_uuid: source.uuid.clone(),
            name: source.name.clone(),
            egg: source.relationships.egg.as_ref().map(|egg| egg.attributes.name.clone()).unwrap_or_default(),
            suspended: source.suspended,
            blueprint_id: image.blueprint_id.to_string(),
            reason: image.reason,
            fields: image.fields,
            ports: plan_ports(source),
            environment: environment_to_carry(source)
                .into_iter()
                .map(|(key, value)| PlannedEnvironmentVariable { key, value })
                .collect(),
            memory_mb,
            cpu_cores,
            databases,
            source: source_location,
            warnings,
        });
    }

    if planned.len() < total_servers as usize {
        notes.push(PlanNote::with("someServersUnreadable", &[("total", &total_servers.to_string()), ("read", &planned.len().to_string())]));
    }
    if known.is_empty() {
        notes.push(PlanNote::new("noNodesYet"));
    }

    Ok(MigrationPlan { panel_url: String::new(), total_servers, servers: planned, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pterodactyl::models::{Allocation, ListMeta, ListResponse, Wrapped};

    fn node(fqdn: &str, base: Option<&str>) -> Node {
        Node { id: 1, name: "wings-01".to_string(), fqdn: fqdn.to_string(), daemon_base: base.map(str::to_string) }
    }

    fn allocation(port: u16, primary: bool) -> Wrapped<Allocation> {
        Wrapped { attributes: Allocation { id: port as i64, ip: "0.0.0.0".to_string(), alias: None, port, notes: None, assigned: primary } }
    }

    #[test]
    fn the_volume_path_follows_the_daemon_the_operator_actually_configured() {
        assert_eq!(volume_path(&node("a", None), "uuid-1"), "/var/lib/pterodactyl/volumes/uuid-1");
        assert_eq!(volume_path(&node("a", Some("/mnt/big-disk/volumes/")), "uuid-1"), "/mnt/big-disk/volumes/uuid-1");
        // An operator who set it to an empty string means "the default", not
        // a path at the filesystem root.
        assert_eq!(volume_path(&node("a", Some("  ")), "uuid-1"), "/var/lib/pterodactyl/volumes/uuid-1");
    }

    /// The primary allocation is the one players connect to. It has to be
    /// obvious in the plan, because it is the port that must survive if any
    /// of them has to change.
    #[test]
    fn the_primary_port_is_listed_first() {
        let mut server = crate::pterodactyl::models::Server {
            id: 1,
            uuid: "u".to_string(),
            identifier: "u".to_string(),
            name: "s".to_string(),
            description: None,
            suspended: false,
            limits: Default::default(),
            node: 1,
            egg: 1,
            container: Default::default(),
            relationships: Default::default(),
        };
        server.relationships.allocations = Some(ListResponse {
            data: vec![allocation(25575, false), allocation(25565, true)],
            meta: ListMeta::default(),
        });

        let ports = plan_ports(&server);
        assert_eq!(ports[0].port, 25565);
        assert!(ports[0].primary);
        assert_eq!(ports[1].port, 25575);
    }

    /// A wrong match here copies files out of the wrong machine, so the rule
    /// is exact-or-nothing.
    #[test]
    fn a_node_matches_a_vibessh_node_only_on_an_exact_address() {
        let known = vec![KnownNode { id: uuid::Uuid::nil(), name: "wings".to_string(), host: "wings-01.example.com".to_string() }];
        assert!(match_node(&node("WINGS-01.example.com", None), &known, &[]).is_some(), "case must not matter");
        assert!(match_node(&node("wings-02.example.com", None), &known, &[]).is_none());
        assert!(match_node(&node("", None), &known, &[]).is_none(), "a node with no address matches nothing");
    }

    /// The case that made this exist: the same machine, known by a DNS name
    /// in the panel and by an IP in VibeSSH. No comparison of the two
    /// strings can ever succeed, and only the operator knows they are one
    /// machine.
    #[test]
    fn an_operators_answer_beats_an_address_that_cannot_match() {
        let id = uuid::Uuid::new_v4();
        let known = vec![KnownNode { id, name: "vps".to_string(), host: "10.0.0.4".to_string() }];
        let panel_node = node("node.svhard.pl", None);

        assert!(match_node(&panel_node, &known, &[]).is_none(), "an IP and a hostname never compare equal");

        let chosen = vec![NodeOverride { fqdn: "node.svhard.pl".to_string(), server_id: id }];
        assert_eq!(match_node(&panel_node, &known, &chosen).map(|node| node.id), Some(id));

        // An override naming a Node that no longer exists must not be
        // believed - it falls back rather than pretending to have matched.
        let stale = vec![NodeOverride { fqdn: "node.svhard.pl".to_string(), server_id: uuid::Uuid::new_v4() }];
        assert!(match_node(&panel_node, &known, &stale).is_none());
    }

    /// A single-box panel records its database host as loopback, which can
    /// never equal any Node's own address.
    #[test]
    fn loopback_is_recognised_however_it_is_written() {
        for address in ["127.0.0.1", "localhost", "LOCALHOST", "::1", "127.0.1.1", "  127.0.0.1  "] {
            assert!(is_loopback(address), "{address} is the same machine");
        }
        for address in ["10.0.0.4", "db.example.com", ""] {
            assert!(!is_loopback(address), "{address} is somewhere else");
        }
    }
}
