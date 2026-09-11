//! A read-only client for Pterodactyl's Application API.
//!
//! **Read-only, and that is a boundary rather than an omission.** The
//! importer's job is to copy a panel into VibeSSH, and every write it might
//! be tempted to make - deleting a migrated server, suspending one, resetting
//! a database password - is destructive against the very system the user is
//! migrating *away from* and would still like to have if the import goes
//! wrong. Servers are stopped through Docker on their own node, where
//! stopping is reversible and visible, not through the panel's API.
//!
//! The key is an Application API key (Admin -> API Credentials in the panel).
//! It never appears in an error, a log line or a command string: an error
//! from here says the panel rejected it, not what was sent.

use reqwest::StatusCode;

use crate::errors::{AppError, AppResult};

use super::models::{ListResponse, Node, Server, ServerDatabase};

/// How long any single call to the panel may take.
///
/// A panel behind a dead reverse proxy hangs rather than refusing, and the
/// setup wizard has a person waiting in front of it.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// A ceiling on pagination, so a panel that reports a nonsensical
/// `total_pages` cannot spin this forever. Fifty pages is 2500 servers at
/// Pterodactyl's default page size - far past any panel this app expects,
/// and still finite.
const MAX_PAGES: u32 = 50;

pub struct PterodactylClient {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl PterodactylClient {
    pub fn new(base_url: &str, api_key: &str) -> AppResult<Self> {
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(AppError::InvalidInput("the panel address has to start with http:// or https://".to_string()));
        }
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|err| AppError::Internal(format!("could not build an HTTP client: {err}")))?;
        Ok(Self { base_url, api_key: api_key.trim().to_string(), http })
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> AppResult<T> {
        let url = format!("{}/api/application{path}", self.base_url);
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|err| {
                // `err` can carry the URL but never the Authorization header,
                // so this is safe to surface - and the URL is the single most
                // useful thing when somebody has typed the panel address
                // wrong.
                AppError::Connection(format!("could not reach the Pterodactyl panel at {}: {err}", self.base_url))
            })?;

        match response.status() {
            StatusCode::OK => response
                .json::<T>()
                .await
                .map_err(|err| AppError::Internal(format!("the panel's answer could not be read: {err}"))),
            // Two answers, two different fixes. A 401 means the key itself
            // was refused; a 403 means the key is fine and simply has no
            // permission ticked for this resource, which is easy to do when
            // creating one and impossible to guess from a message that says
            // "unauthorized".
            StatusCode::UNAUTHORIZED => Err(AppError::PterodactylKeyRejected),
            StatusCode::FORBIDDEN => Err(AppError::PterodactylKeyForbidden {
                resource: path.trim_start_matches('/').split(['/', '?']).next().unwrap_or("servers").to_string(),
            }),
            StatusCode::NOT_FOUND => Err(AppError::NotFound(format!(
                "the panel has no {path}. Check that the address points at the panel itself and not at a page inside it."
            ))),
            other => Err(AppError::Internal(format!("the panel answered {other} for {path}"))),
        }
    }

    /// Follows `total_pages` rather than trusting the first page.
    ///
    /// Pterodactyl pages at 50. A panel with 60 servers that quietly imported
    /// 50 of them would be the worst kind of bug here: everything looks like
    /// it worked, and ten servers are simply gone.
    async fn get_all<T: serde::de::DeserializeOwned>(&self, path: &str, include: &str) -> AppResult<Vec<T>> {
        let mut collected = Vec::new();
        let mut page = 1;
        loop {
            let separator = if path.contains('?') { '&' } else { '?' };
            let query = if include.is_empty() {
                format!("{path}{separator}page={page}")
            } else {
                format!("{path}{separator}include={include}&page={page}")
            };
            let response: ListResponse<T> = self.get(&query).await?;
            let total_pages = response.meta.pagination.total_pages;
            collected.extend(response.data.into_iter().map(|wrapped| wrapped.attributes));

            if page >= total_pages.max(1) || page >= MAX_PAGES {
                if total_pages > MAX_PAGES {
                    log::warn!("the panel reports {total_pages} pages for {path}; stopped after {MAX_PAGES}");
                }
                return Ok(collected);
            }
            page += 1;
        }
    }

    /// Verifies the address and the key before the wizard goes any further,
    /// so a typo is reported at the step where it was made rather than
    /// halfway through an import.
    pub async fn check_access(&self) -> AppResult<u32> {
        let response: ListResponse<Server> = self.get("/servers?page=1").await?;
        Ok(response.meta.pagination.total)
    }

    pub async fn list_servers(&self) -> AppResult<Vec<Server>> {
        self.get_all("/servers", "allocations,egg").await
    }

    pub async fn list_nodes(&self) -> AppResult<Vec<Node>> {
        self.get_all("/nodes", "").await
    }

    /// `include=host` is what carries the database host's *address*. Without
    /// it the rows name only a host id, and an id cannot be matched to a
    /// machine VibeSSH manages.
    pub async fn list_server_databases(&self, server_id: i64) -> AppResult<Vec<ServerDatabase>> {
        self.get_all(&format!("/servers/{server_id}/databases"), "host").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deliberately matched rather than unwrapped: `unwrap_err` would need
    /// `Debug` on the client, and the client holds an API key. A type that
    /// can print itself is a type that can print a secret into a panic
    /// message.
    #[test]
    fn an_address_without_a_scheme_is_refused_before_anything_is_sent() {
        match PterodactylClient::new("panel.example.com", "key") {
            Err(AppError::InvalidInput(_)) => {}
            Err(other) => panic!("expected InvalidInput, got {other:?}"),
            Ok(_) => panic!("an address with no scheme must be refused"),
        }
    }

    #[test]
    fn a_trailing_slash_does_not_produce_a_double_slash_in_the_url() {
        let client = PterodactylClient::new("https://panel.example.com/", "key").unwrap();
        assert_eq!(client.base_url, "https://panel.example.com");
    }

    /// Whitespace around a pasted key is the single most common way to get a
    /// 401 from a key that is in fact correct.
    #[test]
    fn a_pasted_key_is_trimmed() {
        let client = PterodactylClient::new("https://panel.example.com", "  ptla_abc  ").unwrap();
        assert_eq!(client.api_key, "ptla_abc");
    }
}
