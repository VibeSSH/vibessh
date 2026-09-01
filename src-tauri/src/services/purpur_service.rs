//! A client for Purpur's own build API (`api.purpurmc.org/v2/purpur`) - a
//! separate host and shape from PaperMC's Fill API (`papermc_service.rs`),
//! since Purpur is a community fork distributed independently of PaperMC
//! rather than one more project behind the same API. Lists available
//! versions and resolves one to its latest build's download info, same
//! shape as `papermc_service::{list_versions, latest_build}` so
//! `blueprints::PurpurBlueprint` can follow `PaperBlueprint`'s own
//! provisioning pattern unchanged.

use serde::Deserialize;

use crate::errors::{AppError, AppResult};

const BASE_URL: &str = "https://api.purpurmc.org/v2/purpur";

#[derive(Debug, Deserialize)]
struct VersionsResponse {
    versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BuildsResponse {
    builds: BuildsField,
}

#[derive(Debug, Deserialize)]
struct BuildsField {
    latest: String,
}

pub struct PurpurBuild {
    pub filename: String,
    pub url: String,
}

/// Every currently-available Purpur version, newest first. The API already
/// returns them oldest-first with a mix of numeric release lines (`"1.21.4"`)
/// and standalone build-line versions (`"26.2"`) - reuses the same
/// numeric-component comparison `papermc_service::compare_versions` uses,
/// duplicated rather than shared per this crate's own small-helper
/// convention (see `dns_service`'s doc comment on `network_alias`).
pub async fn list_versions() -> AppResult<Vec<String>> {
    let response: VersionsResponse = reqwest::get(BASE_URL)
        .await
        .map_err(|err| AppError::Connection(format!("couldn't reach purpurmc.org: {err}")))?
        .json()
        .await
        .map_err(|err| AppError::Connection(format!("couldn't read purpurmc.org's response: {err}")))?;

    let mut versions = response.versions;
    versions.sort_by(|a, b| compare_versions(b, a));
    Ok(versions)
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    fn parse(version: &str) -> Vec<u32> {
        version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
    }
    parse(a).cmp(&parse(b))
}

/// The latest build for a Purpur version - `filename` mirrors what the
/// download actually names itself (`purpur-<version>-<build>.jar`, verified
/// against the API's own `Content-Disposition` header) rather than parsing
/// that header at download time.
pub async fn latest_build(version: &str) -> AppResult<PurpurBuild> {
    let url = format!("{BASE_URL}/{version}");
    let response = reqwest::get(&url).await.map_err(|err| AppError::Connection(format!("couldn't reach purpurmc.org: {err}")))?;
    if !response.status().is_success() {
        return Err(AppError::InvalidInput(format!("no purpur build found for version '{version}'")));
    }
    let builds: BuildsResponse =
        response.json().await.map_err(|err| AppError::Connection(format!("couldn't read purpurmc.org's response: {err}")))?;

    let build = builds.builds.latest;
    let filename = format!("purpur-{version}-{build}.jar");
    let url = format!("{BASE_URL}/{version}/{build}/download");
    Ok(PurpurBuild { filename, url })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_versions_orders_numeric_components_not_lexically() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("1.9", "1.21"), Ordering::Less);
        assert_eq!(compare_versions("26.2", "1.21.11"), Ordering::Greater);
    }

    #[tokio::test]
    async fn list_versions_reaches_the_real_purpur_api() {
        let Ok(versions) = list_versions().await else {
            eprintln!("skipping: purpurmc.org unreachable from this environment");
            return;
        };
        assert!(!versions.is_empty());
    }

    #[tokio::test]
    async fn latest_build_reaches_the_real_purpur_api_and_returns_a_downloadable_jar_name() {
        let Ok(versions) = list_versions().await else {
            eprintln!("skipping: purpurmc.org unreachable from this environment");
            return;
        };
        let Some(version) = versions.into_iter().next() else {
            return;
        };
        let Ok(build) = latest_build(&version).await else {
            eprintln!("skipping: purpurmc.org unreachable from this environment");
            return;
        };
        assert!(build.filename.starts_with("purpur-"));
        assert!(build.filename.ends_with(".jar"));
        assert!(build.url.ends_with("/download"));
    }
}
