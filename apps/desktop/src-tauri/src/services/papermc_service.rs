//! A client for PaperMC's "Fill" API v3 (`fill.papermc.io`, the current API
//! as of this writing - Paper's older `papermc.io/v2` API has been
//! retired) - not just Paper itself. PaperMC hosts several projects behind
//! the identical API shape (only the project id in the URL and the exact
//! version numbering differ) - Velocity (the proxy) is the other one this
//! crate uses, so every function here takes a `project` id (`"paper"`,
//! `"velocity"`) rather than being hardcoded to one. Lists available
//! versions and resolves one to its latest build's download info. Used by
//! `blueprints::{PaperBlueprint, VelocityBlueprint}` (jar download during
//! provisioning) and by `commands::application_commands::{list_paper_versions,
//! list_velocity_versions}` (the wizard's version picker).

use std::collections::HashMap;

use serde::Deserialize;

use crate::errors::{AppError, AppResult};

fn project_base(project: &str) -> String {
    format!("https://fill.papermc.io/v3/projects/{project}")
}

#[derive(Debug, Deserialize)]
struct ProjectResponse {
    versions: HashMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct BuildResponse {
    downloads: HashMap<String, BuildDownload>,
}

#[derive(Debug, Deserialize)]
struct BuildDownload {
    name: String,
    url: String,
}

pub struct PapermcBuild {
    pub filename: String,
    pub url: String,
}

/// Every currently-available version for a PaperMC project, newest first.
/// The API groups versions under a release-line key (e.g. `"1.21"` ->
/// `["1.21.11", "1.21.11-rc3", ..., "1.21"]`) - this flattens every
/// group's values into one list and drops anything containing
/// `"-rc"`/`"-pre"`/`"-snapshot"` (case-insensitive - Velocity's own
/// unstable builds are suffixed `"-SNAPSHOT"`), since a version picker's
/// default list shouldn't point a new server at an unstable build.
pub async fn list_versions(project: &str) -> AppResult<Vec<String>> {
    let response: ProjectResponse = reqwest::get(project_base(project))
        .await
        .map_err(|err| AppError::Connection(format!("couldn't reach papermc.io: {err}")))?
        .json()
        .await
        .map_err(|err| AppError::Connection(format!("couldn't read papermc.io's response: {err}")))?;

    let mut versions: Vec<String> = response
        .versions
        .into_values()
        .flatten()
        .filter(|version| {
            let lower = version.to_lowercase();
            !lower.contains("-rc") && !lower.contains("-pre") && !lower.contains("-snapshot")
        })
        .collect();
    versions.sort_by(|a, b| compare_versions(b, a));
    Ok(versions)
}

/// Numeric-component comparison (`"1.21.11"` vs `"26.2"` vs `"1.7.10"`) -
/// robust enough for these real version strings without a full semver
/// dependency; a non-numeric component sorts as 0 rather than panicking, so
/// an unexpected format just sorts unhelpfully instead of crashing the
/// whole list.
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    fn parse(version: &str) -> Vec<u32> {
        version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
    }
    parse(a).cmp(&parse(b))
}

/// The latest build for a project's version, whichever channel it's on -
/// there's no separate "latest stable build" endpoint, and `list_versions`
/// already only offers version *strings* that look stable, so the latest
/// build for one of those is the right thing to fetch.
pub async fn latest_build(project: &str, version: &str) -> AppResult<PapermcBuild> {
    let url = format!("{}/versions/{version}/builds/latest", project_base(project));
    let response =
        reqwest::get(&url).await.map_err(|err| AppError::Connection(format!("couldn't reach papermc.io: {err}")))?;
    if !response.status().is_success() {
        return Err(AppError::InvalidInput(format!("no {project} build found for version '{version}'")));
    }
    let build: BuildResponse =
        response.json().await.map_err(|err| AppError::Connection(format!("couldn't read papermc.io's response: {err}")))?;

    let download = build
        .downloads
        .get("server:default")
        .ok_or_else(|| AppError::Internal("papermc.io's response had no server:default download".into()))?;
    Ok(PapermcBuild { filename: download.name.clone(), url: download.url.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_versions_orders_numeric_components_not_lexically() {
        use std::cmp::Ordering;
        // Lexical string comparison would put "1.9" *after* "1.21" (since
        // '9' > '2' as characters) - numerically, 1.9 predates 1.21, so the
        // correct ordering is the other way around.
        assert_eq!(compare_versions("1.9", "1.21"), Ordering::Less);
        assert_eq!(compare_versions("26.2", "1.21.11"), Ordering::Greater);
        assert_eq!(compare_versions("1.21.11", "1.21.4"), Ordering::Greater);
        assert_eq!(compare_versions("1.7.10", "1.7.10"), Ordering::Equal);
    }

    #[tokio::test]
    async fn list_versions_reaches_the_real_paper_api_and_excludes_unstable_builds() {
        // A real network call, same reasoning as this crate's other
        // integration-style tests that exercise the actual external
        // service rather than a mock of assumptions about its shape.
        let Ok(versions) = list_versions("paper").await else {
            eprintln!("skipping: papermc.io unreachable from this environment");
            return;
        };
        assert!(!versions.is_empty());
        assert!(versions.iter().all(|v| {
            let lower = v.to_lowercase();
            !lower.contains("-rc") && !lower.contains("-pre") && !lower.contains("-snapshot")
        }));
    }

    #[tokio::test]
    async fn list_versions_reaches_the_real_velocity_api_and_excludes_snapshots() {
        let Ok(versions) = list_versions("velocity").await else {
            eprintln!("skipping: papermc.io unreachable from this environment");
            return;
        };
        assert!(!versions.is_empty());
        assert!(versions.iter().all(|v| !v.to_lowercase().contains("-snapshot")));
    }
}
