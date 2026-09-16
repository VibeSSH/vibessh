//! The update manifest, served from here so the count stays here.
//!
//! The desktop app asks for `latest.json` five seconds after launch and every
//! six hours after that. It used to ask GitHub, which meant the only party
//! able to say how many installations were running was Microsoft - and all
//! they offer is a download counter, which cannot tell one machine left open
//! from forty opened once.
//!
//! So this answers instead, with the same file, and records that somebody
//! asked. **The app sends nothing here it was not already sending to
//! GitHub**: an address, because every HTTP request has one, and its version
//! and platform, which the updater puts in the query string so it can be
//! given the right manifest.
//!
//! **What is kept.** A day, a hash, a version, a platform, a count. The hash
//! is of the address together with a salt that changes daily, so the same
//! machine is one row within a day and is unlinkable across days - see
//! `migrations/0013_update_checks.sql` for why that trade is the right one
//! and what it costs.
//!
//! **The day-by-day numbers are not readable over HTTP.** Anyone entitled to
//! that breakdown already has SSH to this machine, and a route for it would
//! have meant a token to generate, hand over and keep safe.
//! `apps/backend/scripts/update-stats.sh` reads the table directly and ships
//! with every deploy.
//!
//! `stats` is the one exception, and only because it is not a secret: it
//! answers with the two figures the landing page prints. It needs no token
//! precisely because everything in it is about to be published on a public
//! web page - there is nothing there to protect.
//!
//! **This must never be able to stop an update.** An update mechanism that
//! fails closed because a counter had a bad day is worse than no counter, so
//! every failure here - the database, the fetch, the cache - ends with the
//! manifest still being served or the caller being sent to GitHub, never
//! with an error the updater would show to somebody.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::AppState;

/// Where the file really lives. This service is a counter in front of it, not
/// a second source of truth: the release workflow publishes there and nothing
/// here can disagree with it.
const UPSTREAM: &str = "https://github.com/VibeSSH/vibessh-releases/releases/latest/download/latest.json";

/// How long a fetched manifest is reused.
///
/// The updater asks every six hours per installation, so without this every
/// running copy would turn into a request to GitHub. Five minutes keeps a new
/// release visible almost immediately while collapsing a burst of checks into
/// one fetch.
const CACHE_FOR: Duration = Duration::from_secs(5 * 60);

/// Every release with its assets, which is where a download count lives.
///
/// `per_page=100` rather than paging: there are fifteen releases, and a
/// counter that silently stopped counting the day there were a hundred and
/// one would be worse than one openly capped at the most recent hundred,
/// which is what this is.
const RELEASES_API: &str = "https://api.github.com/repos/VibeSSH/vibessh-releases/releases?per_page=100";

/// What a person downloads, as opposed to what a machine downloads.
///
/// A release carries fifteen assets and the raw total across them means
/// nothing as a measure of people: measured on the real releases it came to
/// 352, of which 204 were `latest.json`, fetched by every running copy every
/// six hours, and another 21 were `.sig` files the updater fetches beside an
/// installer nobody chose to download. These four are what a human clicks,
/// so these four are what gets published as "downloads".
const INSTALLER_SUFFIXES: [&str; 4] = ["-setup.exe", ".AppImage", ".deb", ".tar.gz"];

/// How long the public figures are reused.
///
/// Long, because this is a headline on a web page rather than a dashboard,
/// and because the alternative is one GitHub API call per visitor against an
/// unauthenticated limit of sixty an hour for the whole server. Four calls an
/// hour leaves that limit alone.
const STATS_CACHE_FOR: Duration = Duration::from_secs(15 * 60);

/// The cached manifest, and when it was fetched.
#[derive(Default)]
pub struct ManifestCache {
    inner: Mutex<Option<(Instant, String)>>,
}

impl ManifestCache {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    async fn get(&self) -> Option<String> {
        let guard = self.inner.lock().await;
        guard.as_ref().filter(|(at, _)| at.elapsed() < CACHE_FOR).map(|(_, body)| body.clone())
    }

    async fn put(&self, body: String) {
        *self.inner.lock().await = Some((Instant::now(), body));
    }
}

/// What the updater tells us about itself, straight from the query string
/// Tauri appends. Both are labels for grouping and neither is trusted.
#[derive(Deserialize)]
pub struct CheckQuery {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
}

/// Keeps a label short and boring before it reaches the database.
///
/// These arrive from anybody who can make an HTTP request, so they are cut to
/// a length and a character set rather than stored as sent - a version string
/// is digits and dots, and a megabyte of anything is not a version.
fn label(value: Option<String>) -> Option<String> {
    let value = value?;
    let cleaned: String = value.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).take(40).collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// The salt for today, made on first use and read from the table afterwards.
///
/// In the database rather than in memory so a restart does not split one day
/// into two sets of hashes that no longer collide - which would quietly
/// inflate the only number this table exists to produce.
async fn salt_for(db: &sqlx::PgPool, day: NaiveDate) -> Result<Vec<u8>, sqlx::Error> {
    if let Some((salt,)) = sqlx::query_as::<_, (Vec<u8>,)>("SELECT salt FROM update_check_salts WHERE day = $1")
        .bind(day)
        .fetch_optional(db)
        .await?
    {
        return Ok(salt);
    }

    let fresh: [u8; 32] = rand::random();
    // `DO NOTHING` then read back: two first-requests of the day can race,
    // and the one that loses must use the salt that won rather than its own.
    sqlx::query("INSERT INTO update_check_salts (day, salt) VALUES ($1, $2) ON CONFLICT (day) DO NOTHING")
        .bind(day)
        .bind(&fresh[..])
        .execute(db)
        .await?;
    let (salt,) = sqlx::query_as::<_, (Vec<u8>,)>("SELECT salt FROM update_check_salts WHERE day = $1").bind(day).fetch_one(db).await?;
    Ok(salt)
}

/// Writes down that somebody checked, and never fails the caller.
///
/// Errors are logged and dropped on purpose: this is a counter attached to an
/// update mechanism, and an update that stopped working because a counter
/// could not write would be a bad trade in every direction.
async fn record(state: &AppState, address: &str, version: Option<String>, platform: Option<String>) {
    let now = Utc::now();
    let day = now.date_naive();

    let salt = match salt_for(&state.db, day).await {
        Ok(salt) => salt,
        Err(err) => {
            log::warn!("couldn't read today's update-check salt: {err}");
            return;
        }
    };

    let mut hasher = Sha256::new();
    hasher.update(&salt);
    hasher.update(address.as_bytes());
    let client_day_hash = hasher.finalize().to_vec();

    let written = sqlx::query(
        "INSERT INTO update_checks (day, client_day_hash, version, platform, checks, last_seen_at) \
         VALUES ($1, $2, $3, $4, 1, $5) \
         ON CONFLICT (day, client_day_hash, version, platform) \
         DO UPDATE SET checks = update_checks.checks + 1, last_seen_at = $5",
    )
    .bind(day)
    .bind(&client_day_hash)
    .bind(version.unwrap_or_else(|| "unknown".to_string()))
    .bind(platform.unwrap_or_else(|| "unknown".to_string()))
    .bind(now)
    .execute(&state.db)
    .await;

    if let Err(err) = written {
        log::warn!("couldn't record an update check: {err}");
    }
}

/// Serves the manifest, and counts the asking.
///
/// A fetch that fails sends the caller to GitHub instead of answering with an
/// error. The updater then gets exactly what it would have got before this
/// endpoint existed, which is the whole point: nothing here may be a reason
/// an update does not happen.
pub async fn latest(State(state): State<AppState>, headers: HeaderMap, Query(query): Query<CheckQuery>) -> Response {
    // The same header the rate limiter reads, and the same rule about
    // trusting it - see `rate_limit::client_key`.
    let forwarded = headers.get("cf-connecting-ip").or_else(|| headers.get("x-forwarded-for")).and_then(|value| value.to_str().ok());
    let address = crate::rate_limit::client_key(forwarded, None);

    let platform = label(query.target.clone()).map(|target| match label(query.arch.clone()) {
        Some(arch) => format!("{target}-{arch}"),
        None => target,
    });
    record(&state, &address, label(query.version), platform).await;

    if let Some(cached) = state.manifest_cache.get().await {
        return ([(axum::http::header::CONTENT_TYPE, "application/json")], cached).into_response();
    }

    match fetch_upstream().await {
        Ok(body) => {
            state.manifest_cache.put(body.clone()).await;
            ([(axum::http::header::CONTENT_TYPE, "application/json")], body).into_response()
        }
        Err(err) => {
            log::warn!("couldn't fetch the update manifest, sending the caller upstream: {err}");
            Redirect::temporary(UPSTREAM).into_response()
        }
    }
}

async fn fetch_upstream() -> Result<String, String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().map_err(|err| err.to_string())?;
    let response = client.get(UPSTREAM).send().await.map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("upstream answered {}", response.status()));
    }
    response.text().await.map_err(|err| err.to_string())
}

/// The two public figures, as the landing page receives them.
///
/// Both are `null` rather than `0` when they could not be worked out, so the
/// page can tell "nobody yet" from "we do not know" and show neither a wrong
/// number nor a zero that reads like failure.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// Installers downloaded from GitHub, across every release, ever.
    pub downloads: Option<i64>,
    /// Distinct installations that checked for an update on `day`.
    ///
    /// One complete day, never a range: the hash that makes a machine one row
    /// within a day is deliberately not comparable across days, so adding two
    /// days together would count the same machine twice and calling that
    /// "users this week" would be a lie the table itself cannot detect.
    pub installations: Option<i64>,
    /// Which day `installations` is for - yesterday, because today is half
    /// over and a partial day shown as a total always reads low.
    pub day: Option<NaiveDate>,
}

#[derive(Default)]
pub struct StatsCache {
    inner: Mutex<Option<(Instant, Stats)>>,
}

impl StatsCache {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[derive(Deserialize)]
pub struct ApiRelease {
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    download_count: i64,
}

/// Adds up the assets a person would have clicked.
fn installer_downloads(releases: &[ApiRelease]) -> i64 {
    releases
        .iter()
        .flat_map(|release| release.assets.iter())
        .filter(|asset| INSTALLER_SUFFIXES.iter().any(|suffix| asset.name.ends_with(suffix)))
        .map(|asset| asset.download_count)
        .sum()
}

async fn fetch_downloads() -> Result<i64, String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().map_err(|err| err.to_string())?;
    let response = client
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        // GitHub refuses an API request without one, and a name is more use
        // to whoever reads their logs than a default.
        .header("User-Agent", "vibessh-backend")
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("github answered {}", response.status()));
    }
    let releases: Vec<ApiRelease> = response.json().await.map_err(|err| err.to_string())?;
    Ok(installer_downloads(&releases))
}

/// The numbers for the landing page.
///
/// Readable by anybody, deliberately: this is what the site prints. It
/// records nothing about the caller - `latest` is the app checking in, this
/// is a person reading a page, and counting readers here would put web
/// analytics into a table whose whole design is about not being that.
pub async fn stats(State(state): State<AppState>) -> Response {
    let cached = {
        let guard = state.stats_cache.inner.lock().await;
        guard.as_ref().filter(|(at, _)| at.elapsed() < STATS_CACHE_FOR).map(|(_, stats)| stats.clone())
    };
    if let Some(stats) = cached {
        return public(stats);
    }

    let downloads = match fetch_downloads().await {
        Ok(count) => Some(count),
        Err(err) => {
            log::warn!("couldn't read the download count from GitHub: {err}");
            None
        }
    };

    let day = Utc::now().date_naive().pred_opt();
    let installations = match day {
        Some(day) => match sqlx::query_scalar::<_, i64>("SELECT count(DISTINCT client_day_hash) FROM update_checks WHERE day = $1")
            .bind(day)
            .fetch_one(&state.db)
            .await
        {
            Ok(count) => Some(count),
            Err(err) => {
                log::warn!("couldn't count yesterday's update checks: {err}");
                None
            }
        },
        None => None,
    };

    let stats = Stats { downloads, installations, day };
    // Cached even when a number is missing, so an outage at GitHub costs one
    // failed call every fifteen minutes rather than one per visitor.
    *state.stats_cache.inner.lock().await = Some((Instant::now(), stats.clone()));
    public(stats)
}

/// `*` because the answer is public, and because `*` is the setting that
/// cannot be used to read anything private: a browser will not attach
/// cookies or an `Authorization` header to a request whose response allows
/// any origin, so this cannot become a way into the rest of the API.
fn public(stats: Stats) -> Response {
    ([(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")], axum::Json(stats)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These arrive from anybody who can make a request, so a version is
    /// whatever somebody typed until it is cut down to something boring.
    #[test]
    fn a_label_is_trimmed_to_something_that_can_only_be_a_label() {
        assert_eq!(label(Some("0.1.0-beta.14".to_string())), Some("0.1.0-beta.14".to_string()));
        assert_eq!(label(Some("windows-x86_64".to_string())), Some("windows-x86_64".to_string()));
        // Anything else is dropped rather than stored and shown later.
        assert_eq!(label(Some("<script>alert(1)</script>".to_string())), Some("scriptalert1script".to_string()));
        assert_eq!(label(Some("x".repeat(500))), Some("x".repeat(40)));
        assert_eq!(label(Some("   ".to_string())), None);
        assert_eq!(label(None), None);
    }

    /// The property the whole design rests on: the same machine on two days
    /// produces two hashes that cannot be matched to each other.
    #[test]
    fn the_same_address_hashes_differently_under_a_different_days_salt() {
        let hash = |salt: &[u8], address: &str| {
            let mut hasher = Sha256::new();
            hasher.update(salt);
            hasher.update(address.as_bytes());
            hasher.finalize().to_vec()
        };
        let today = [1u8; 32];
        let tomorrow = [2u8; 32];

        // Same day, same machine: one row.
        assert_eq!(hash(&today, "ip:203.0.113.7"), hash(&today, "ip:203.0.113.7"));
        // Different day: nothing links them.
        assert_ne!(hash(&today, "ip:203.0.113.7"), hash(&tomorrow, "ip:203.0.113.7"));
        // Same day, different machines: still distinguishable, which is what
        // makes a daily count mean anything.
        assert_ne!(hash(&today, "ip:203.0.113.7"), hash(&today, "ip:203.0.113.8"));
    }

    /// The published figure says "downloads", and a reader takes that to mean
    /// people. The raw total over every asset does not mean that at all.
    #[test]
    fn only_the_files_a_person_would_click_are_counted_as_downloads() {
        let json = serde_json::json!([{
            "assets": [
                { "name": "VibeSSH_0.1.0-beta.15_x64-setup.exe", "download_count": 20 },
                { "name": "VibeSSH_0.1.0-beta.15_amd64.AppImage", "download_count": 5 },
                { "name": "VibeSSH_0.1.0-beta.15_amd64.deb", "download_count": 3 },
                { "name": "VibeSSH_0.1.0-beta.15_amd64.tar.gz", "download_count": 2 },
                // Every running copy, every six hours. Not a download.
                { "name": "latest.json", "download_count": 900 },
                // Fetched by the updater beside the installer, so counting it
                // would roughly double the Windows figure.
                { "name": "VibeSSH_0.1.0-beta.15_x64-setup.exe.sig", "download_count": 18 },
                { "name": "VibeSSH_0.1.0-beta.15_amd64.deb.sig", "download_count": 3 },
                // The agent is put on a server by a script, and is not
                // somebody downloading VibeSSH.
                { "name": "vibe-agent-linux-amd64", "download_count": 40 },
                { "name": "install.sh", "download_count": 14 }
            ]
        }]);
        let releases: Vec<ApiRelease> = serde_json::from_value(json).unwrap();
        assert_eq!(installer_downloads(&releases), 30);
    }
}
