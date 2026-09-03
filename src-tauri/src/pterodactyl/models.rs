//! The slice of Pterodactyl's Application API this app reads.
//!
//! Deliberately partial. Pterodactyl returns a great deal per server -
//! subusers, backups, schedules, database hosts, the egg's full script - and
//! deserialising all of it would mean this file breaking every time the panel
//! adds a field. Every struct here is `#[serde(default)]`-friendly and names
//! only what the importer actually maps, so an unknown field is ignored
//! rather than fatal.
//!
//! **Every response is wrapped.** The panel answers `{"object": "...",
//! "attributes": {...}}` for one thing and `{"object": "list", "data": [...],
//! "meta": {...}}` for many, so those two envelopes are types here rather
//! than being unwrapped by hand at each call site.

use serde::Deserialize;

/// `{"object": "server", "attributes": {...}}`
#[derive(Debug, Clone, Deserialize)]
pub struct Wrapped<T> {
    pub attributes: T,
}

/// `{"object": "list", "data": [...], "meta": {"pagination": {...}}}`
#[derive(Debug, Clone, Deserialize)]
pub struct ListResponse<T> {
    pub data: Vec<Wrapped<T>>,
    #[serde(default)]
    pub meta: ListMeta,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListMeta {
    #[serde(default)]
    pub pagination: Pagination,
}

/// Pterodactyl pages at 50 by default and will not hand over everything at
/// once, so the client has to follow `total_pages`. A panel with 60 servers
/// silently importing 50 of them is precisely the kind of quiet loss this
/// feature must not have.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Pagination {
    #[serde(default)]
    pub current_page: u32,
    #[serde(default)]
    pub total_pages: u32,
    #[serde(default)]
    pub total: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    pub id: i64,
    /// The long uuid. This is the directory name under the daemon's volume
    /// root, which is how the importer finds the server's files on disk.
    pub uuid: String,
    /// The short id shown in the panel's URLs.
    #[serde(default)]
    pub identifier: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub limits: Limits,
    /// The node this server runs on - which machine its files are on.
    pub node: i64,
    /// The egg it was created from. Mapped to a VibeSSH image.
    pub egg: i64,
    #[serde(default)]
    pub container: Container,
    #[serde(default)]
    pub relationships: ServerRelationships,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Limits {
    /// Megabytes. `0` means unlimited in Pterodactyl, which is not the same
    /// as "unset" and must not be copied across as a limit of zero.
    #[serde(default)]
    pub memory: i64,
    /// Percent of one core, so `200` is two cores. VibeSSH counts cores.
    #[serde(default)]
    pub cpu: i64,
    #[serde(default)]
    pub disk: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Container {
    #[serde(default)]
    pub startup_command: String,
    /// The Docker image the panel runs it in. The strongest single signal
    /// for which VibeSSH image this is, and often the only one that names a
    /// Java version.
    #[serde(default)]
    pub image: String,
    /// The egg's variables with the values this server actually uses.
    #[serde(default)]
    pub environment: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ServerRelationships {
    #[serde(default)]
    pub allocations: Option<ListResponse<Allocation>>,
    #[serde(default)]
    pub egg: Option<Wrapped<Egg>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Allocation {
    pub id: i64,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub alias: Option<String>,
    pub port: u16,
    #[serde(default)]
    pub notes: Option<String>,
    /// Pterodactyl's "primary" allocation for the server. The one a
    /// Minecraft client connects to, and the one whose port is worth
    /// keeping.
    #[serde(default)]
    pub assigned: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Egg {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    pub id: i64,
    pub name: String,
    /// The hostname the panel talks to the daemon on. Used to work out
    /// whether a VibeSSH Node and this Pterodactyl node are the same
    /// machine.
    #[serde(default)]
    pub fqdn: String,
    /// Where the daemon keeps its data. Volumes live in `<daemon_base>` -
    /// read rather than assumed, because an operator who moved it to a
    /// bigger disk is exactly the person whose files must still be found.
    #[serde(default)]
    pub daemon_base: Option<String>,
}

/// One database Pterodactyl created for a server.
///
/// **There is no password here, and that is not an oversight in this
/// struct.** The Application API never returns one: the panel stores it
/// encrypted and only ever shows it through the client API to a user who
/// owns the server. So the importer cannot connect *as* this user, and gets
/// the data out through the database host itself instead.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerDatabase {
    pub id: i64,
    /// The real schema name on the database server, e.g. `s3_survival`.
    pub database: String,
    pub username: String,
    /// Which hosts the panel allowed this user to connect from.
    #[serde(default)]
    pub remote: String,
    /// The database host's id. Only an id - the address arrives in
    /// `relationships` below, and only when it was asked for.
    #[serde(default)]
    pub host: i64,
    #[serde(default)]
    pub relationships: ServerDatabaseRelationships,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ServerDatabaseRelationships {
    /// Present when the request asked for `?include=host`. Optional rather
    /// than assumed, so a panel that does not return it leaves the importer
    /// saying "which machine is unknown" instead of failing outright.
    #[serde(default)]
    pub host: Option<Wrapped<DatabaseHost>>,
}

/// One database *host* the panel provisions databases on.
///
/// Needed because a panel's MySQL is not always on the machine its servers'
/// files are on, and assuming it is would mean taking a dump on a machine
/// that has no such database.
///
/// **There is no `/api/application/databases` endpoint to fetch these from.**
/// Pterodactyl exposes database hosts in its admin interface only; the
/// Application API surfaces one just as a relationship on a server's own
/// database rows, which is where this is read from. Asking for the
/// collection directly returns 404 - and did, until this was corrected.
///
/// `password` is absent for the same reason it is absent from
/// `ServerDatabase`: the API does not return it.
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseHost {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    /// The address the panel connects to it on.
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real answer, trimmed. What matters is that the two
    /// envelopes and the nested relationship list all deserialise together -
    /// that nesting is where a hand-written unwrap would go wrong.
    const SERVERS_PAGE: &str = r#"{
      "object": "list",
      "data": [
        {
          "object": "server",
          "attributes": {
            "id": 3,
            "uuid": "1a7ce997-259b-452e-8b4e-cecc464142ca",
            "identifier": "1a7ce997",
            "name": "Survival",
            "description": "",
            "suspended": false,
            "limits": { "memory": 4096, "swap": 0, "disk": 10240, "io": 500, "cpu": 200 },
            "node": 1,
            "egg": 5,
            "container": {
              "startup_command": "java -Xms128M -Xmx{{SERVER_MEMORY}}M -jar {{SERVER_JARFILE}}",
              "image": "ghcr.io/pterodactyl/yolks:java_17",
              "environment": { "SERVER_JARFILE": "server.jar", "MINECRAFT_VERSION": "1.20.1" }
            },
            "relationships": {
              "allocations": {
                "object": "list",
                "data": [
                  { "object": "allocation", "attributes": { "id": 11, "ip": "0.0.0.0", "alias": null, "port": 25565, "notes": null, "assigned": true } },
                  { "object": "allocation", "attributes": { "id": 12, "ip": "0.0.0.0", "alias": null, "port": 25575, "notes": null, "assigned": false } }
                ]
              },
              "egg": { "object": "egg", "attributes": { "id": 5, "name": "Paper", "description": "High performance Spigot fork" } }
            }
          }
        }
      ],
      "meta": { "pagination": { "total": 1, "count": 1, "per_page": 50, "current_page": 1, "total_pages": 1 } }
    }"#;

    #[test]
    fn a_servers_page_deserialises_with_its_nested_relationships() {
        let page: ListResponse<Server> = serde_json::from_str(SERVERS_PAGE).expect("the fixture must parse");
        assert_eq!(page.meta.pagination.total_pages, 1);

        let server = &page.data[0].attributes;
        assert_eq!(server.name, "Survival");
        assert_eq!(server.uuid, "1a7ce997-259b-452e-8b4e-cecc464142ca");
        assert_eq!(server.limits.memory, 4096);
        assert_eq!(server.container.image, "ghcr.io/pterodactyl/yolks:java_17");

        let allocations = server.relationships.allocations.as_ref().expect("allocations were included");
        assert_eq!(allocations.data.len(), 2);
        assert!(allocations.data[0].attributes.assigned, "the primary allocation must be recognisable");

        assert_eq!(server.relationships.egg.as_ref().expect("the egg was included").attributes.name, "Paper");
    }

    /// The database host arrives as a relationship on a server database,
    /// and only when `include=host` asked for it.
    ///
    /// This test exists because the address was first looked for in a
    /// `/api/application/databases` collection, which Pterodactyl does not
    /// have - the panel answered 404 and the whole plan failed. The shape
    /// below is the one that does exist.
    #[test]
    fn a_server_database_carries_its_host_address_in_the_included_relationship() {
        let json = r#"{
          "object": "list",
          "data": [
            {
              "object": "server_database",
              "attributes": {
                "id": 4,
                "server": 3,
                "host": 1,
                "database": "s3_survival",
                "username": "u3_QmFy",
                "remote": "%",
                "relationships": {
                  "host": {
                    "object": "database_host",
                    "attributes": { "id": 1, "name": "local mariadb", "host": "10.0.0.4", "port": 3306, "username": "pterodactyl" }
                  }
                }
              }
            }
          ],
          "meta": { "pagination": { "total": 1, "count": 1, "per_page": 50, "current_page": 1, "total_pages": 1 } }
        }"#;

        let page: ListResponse<ServerDatabase> = serde_json::from_str(json).expect("the fixture must parse");
        let database = &page.data[0].attributes;
        assert_eq!(database.database, "s3_survival");
        let host = database.relationships.host.as_ref().expect("the host was included");
        assert_eq!(host.attributes.host, "10.0.0.4", "the address is the whole reason for including it");
        assert_eq!(host.attributes.port, 3306);
    }

    /// Without `include=host` the row names only an id. That has to read as
    /// "not told", never as "no host".
    #[test]
    fn a_server_database_without_the_included_host_still_parses() {
        let json = r#"{"object":"server_database","attributes":{"id":4,"host":1,"database":"s3_x","username":"u3_x"}}"#;
        let wrapped: Wrapped<ServerDatabase> = serde_json::from_str(json).unwrap();
        assert_eq!(wrapped.attributes.host, 1);
        assert!(wrapped.attributes.relationships.host.is_none());
    }

    /// The shape the database migration depends on entirely.
    ///
    /// `/api/application/databases` does not exist, so the host address is
    /// only ever available here, as a relationship, and only when the request
    /// asked for it. Getting this wrong does not fail loudly - it silently
    /// leaves every database looking like it lives on an unknown machine - so
    /// Without `?include=host` the row still parses and simply says nothing
    /// A panel that returns a field this app has never heard of must not
    /// stop an import. Pterodactyl adds fields between minor versions and
    /// every one of them would otherwise be a crash.
    #[test]
    fn an_unknown_field_is_ignored_rather_than_fatal() {
        let json = r#"{
          "object": "server",
          "attributes": {
            "id": 1, "uuid": "u", "name": "n", "node": 1, "egg": 1,
            "some_field_from_a_newer_panel": { "nested": true }
          }
        }"#;
        let wrapped: Wrapped<Server> = serde_json::from_str(json).expect("unknown fields must be ignored");
        assert_eq!(wrapped.attributes.name, "n");
    }

    /// Relationships are only present when the caller asked for them with
    /// `?include=`. Absent must read as "not asked for", never as "none".
    #[test]
    fn a_server_without_included_relationships_still_parses() {
        let json = r#"{"object":"server","attributes":{"id":1,"uuid":"u","name":"n","node":1,"egg":1}}"#;
        let wrapped: Wrapped<Server> = serde_json::from_str(json).unwrap();
        assert!(wrapped.attributes.relationships.allocations.is_none());
        assert!(wrapped.attributes.relationships.egg.is_none());
    }
}
