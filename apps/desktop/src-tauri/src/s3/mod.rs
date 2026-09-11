//! Minimal AWS SigV4 client for S3-compatible object storage (AWS S3,
//! Cloudflare R2, MinIO, ...) - see `Cargo.toml`'s own comment on why this
//! is hand-rolled against `hmac`/`sha2` rather than the official
//! `aws-sdk-s3`. Only the four operations `services::application_backup_service`
//! actually needs: put/get/delete a single object, each a plain,
//! non-multipart request - matches `files::archive::create_zip`'s own
//! "fine for the file sizes this UI already handles, not multi-gigabyte
//! datasets" scope, not a general-purpose S3 SDK.
//!
//! **Signing is verified against an independently-computed reference
//! vector**, not just "looks right" - see `tests::sign_matches_an_independently_computed_reference_vector`,
//! whose expected canonical request/string-to-sign/signature were computed
//! once via Python's own `hashlib`/`hmac` (a separate, trusted
//! implementation) for a fixed, arbitrary request, then pinned here. A
//! signing bug means every upload/download/delete fails loudly (403/404),
//! not silent data corruption - but wrong output from hand-rolled crypto
//! is exactly the kind of mistake that's easy to make and easy to miss by
//! eye, so it's pinned against real independently-derived output instead
//! of trusted on inspection alone.

use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::errors::{AppError, AppResult};
use crate::models::BackupDestinationConfig;

type HmacSha256 = Hmac<Sha256>;

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts a key of any length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// SigV4's own `UriEncode()`: every byte except the unreserved set passes
/// through unchanged, everything else becomes `%XX` with uppercase hex
/// digits - see this module's own doc comment on why deviating from a
/// platform URL-encoder here (which usually encodes a different, slightly
/// smaller "safe" set, and rarely uppercases hex digits) actually matters:
/// a differently-encoded canonical request produces a completely different,
/// silently-wrong signature, not a merely-imperfect one.
fn uri_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Encodes an object key for use in a URL path - each `/`-separated
/// segment is `uri_encode`d on its own, then rejoined with unencoded `/`
/// separators (SigV4's own rule: "Encode the forward slash character
/// everywhere except in the object key name").
fn encode_key_path(key: &str) -> String {
    key.split('/').map(uri_encode).collect::<Vec<_>>().join("/")
}

pub struct S3Client {
    config: BackupDestinationConfig,
    secret_access_key: String,
    http: reqwest::Client,
}

impl S3Client {
    pub fn new(config: BackupDestinationConfig, secret_access_key: String) -> Self {
        Self { config, secret_access_key, http: reqwest::Client::new() }
    }

    /// `self.config.path_prefix` (e.g. `"vibessh-backups"`) sits in front of
    /// every key this client touches - callers pass the same short,
    /// unprefixed key `services::application_backup_service` already uses
    /// to name the local file (`{application_id}/{file_name}`), never a
    /// full path they computed themselves.
    fn prefixed_key(&self, key: &str) -> String {
        let prefix = self.config.path_prefix.trim().trim_matches('/');
        if prefix.is_empty() { key.to_string() } else { format!("{prefix}/{key}") }
    }

    /// `(host, path, full URL)` for `key` - path-style
    /// (`endpoint/bucket/key`, what MinIO needs) or virtual-hosted-style
    /// (`bucket.endpoint-host/key`, what AWS S3/R2 both prefer) depending
    /// on `self.config.path_style`.
    /// `true` for an endpoint host that cannot leave the machine - the one
    /// case where plain HTTP is not a disclosure.
    fn is_loopback_host_part(host: &str) -> bool {
        let host = host.split('/').next().unwrap_or(host);
        let host = host.rsplit_once(':').map_or(host, |(before, _)| before);
        matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
    }

    fn request_target(&self, key: &str) -> AppResult<(String, String, String)> {
        let endpoint = self.config.endpoint.trim().trim_end_matches('/');
        let (scheme, rest) = endpoint
            .split_once("://")
            .ok_or_else(|| AppError::InvalidInput("the backup destination endpoint must start with https://".into()))?;
        // Backups carry the operator's application data, and every request
        // to this endpoint is signed with the destination's secret access
        // key. Over plain HTTP both are on the wire in the clear. The one
        // legitimate exception is a MinIO instance on the same host, so
        // loopback is still allowed - anything else has to be TLS.
        if scheme == "http" && !Self::is_loopback_host_part(rest) {
            return Err(AppError::InvalidInput(
                "the backup destination must use https:// - over plain http the backup contents and the access key are sent in the clear".into(),
            ));
        }
        let encoded_key = encode_key_path(key);
        if self.config.path_style {
            let path = format!("/{}/{encoded_key}", self.config.bucket);
            Ok((rest.to_string(), path.clone(), format!("{scheme}://{rest}{path}")))
        } else {
            let host = format!("{}.{rest}", self.config.bucket);
            let path = format!("/{encoded_key}");
            Ok((host.clone(), path.clone(), format!("{scheme}://{host}{path}")))
        }
    }

    /// Builds the `Authorization` header value for one request - see
    /// `tests::sign_matches_an_independently_computed_reference_vector` for
    /// why this exact shape (three headers signed: `host`,
    /// `x-amz-content-sha256`, `x-amz-date` - the minimum SigV4 requires
    /// plus what S3 itself always requires) is trustworthy.
    fn sign(&self, method: &str, path: &str, host: &str, payload_hash: &str, amz_date: &str, date_stamp: &str) -> String {
        let canonical_headers = format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!("{method}\n{path}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}");
        let hashed_canonical_request = hex_sha256(canonical_request.as_bytes());

        let credential_scope = format!("{date_stamp}/{}/s3/aws4_request", self.config.region);
        let string_to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{hashed_canonical_request}");

        let k_date = hmac_sha256(format!("AWS4{}", self.secret_access_key).as_bytes(), date_stamp.as_bytes());
        let k_region = hmac_sha256(&k_date, self.config.region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        format!("AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}", self.config.access_key_id)
    }

    fn dated_headers(&self) -> (String, String) {
        let now = Utc::now();
        (now.format("%Y%m%dT%H%M%SZ").to_string(), now.format("%Y%m%d").to_string())
    }

    pub async fn put_object(&self, key: &str, bytes: &[u8]) -> AppResult<()> {
        let key = self.prefixed_key(key);
        let (host, path, url) = self.request_target(&key)?;
        let payload_hash = hex_sha256(bytes);
        let (amz_date, date_stamp) = self.dated_headers();
        let authorization = self.sign("PUT", &path, &host, &payload_hash, &amz_date, &date_stamp);

        let response = self
            .http
            .put(&url)
            .header("host", &host)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date)
            .header("authorization", authorization)
            .body(bytes.to_vec())
            .send()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't reach the backup destination: {err}")))?;
        Self::require_success(response, "upload the backup to").await
    }

    pub async fn get_object(&self, key: &str) -> AppResult<Vec<u8>> {
        let key = self.prefixed_key(key);
        let (host, path, url) = self.request_target(&key)?;
        let payload_hash = hex_sha256(b"");
        let (amz_date, date_stamp) = self.dated_headers();
        let authorization = self.sign("GET", &path, &host, &payload_hash, &amz_date, &date_stamp);

        let response = self
            .http
            .get(&url)
            .header("host", &host)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date)
            .header("authorization", authorization)
            .send()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't reach the backup destination: {err}")))?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(AppError::Connection(format!("the backup destination doesn't have this object ({status})")));
        }
        response.bytes().await.map(|bytes| bytes.to_vec()).map_err(|err| AppError::Connection(format!("couldn't download the object: {err}")))
    }

    /// A 404 counts as success here (same "already gone either way" stance
    /// `application_backup_service::delete_backup`'s own best-effort local
    /// delete already takes) - the caller wants the object gone, and it is,
    /// regardless of whether this call is what removed it.
    pub async fn delete_object(&self, key: &str) -> AppResult<()> {
        let key = self.prefixed_key(key);
        let (host, path, url) = self.request_target(&key)?;
        let payload_hash = hex_sha256(b"");
        let (amz_date, date_stamp) = self.dated_headers();
        let authorization = self.sign("DELETE", &path, &host, &payload_hash, &amz_date, &date_stamp);

        let response = self
            .http
            .delete(&url)
            .header("host", &host)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date)
            .header("authorization", authorization)
            .send()
            .await
            .map_err(|err| AppError::Connection(format!("couldn't reach the backup destination: {err}")))?;
        if response.status().is_success() || response.status().as_u16() == 404 {
            return Ok(());
        }
        Err(AppError::Connection(format!("couldn't delete the object from the backup destination ({})", response.status())))
    }

    async fn require_success(response: reqwest::Response, action: &str) -> AppResult<()> {
        if response.status().is_success() {
            return Ok(());
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(300).collect();
        Err(AppError::Connection(format!("the backup destination refused to {action} it ({status}): {snippet}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BackupDestinationConfig {
        BackupDestinationConfig {
            enabled: true,
            endpoint: "https://s3.amazonaws.com".to_string(),
            region: "us-east-1".to_string(),
            bucket: "my-bucket".to_string(),
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            path_prefix: String::new(),
            path_style: false,
        }
    }

    /// Every value below (the canonical request, the string to sign, the
    /// signature) was computed once via Python's own `hashlib`/`hmac` for
    /// this exact request, then pinned here as a reference vector - see
    /// this module's own doc comment for why that's a stronger check than
    /// "the code compiles and the shape looks right."
    #[test]
    fn sign_matches_an_independently_computed_reference_vector() {
        let client = S3Client::new(test_config(), "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string());
        let payload_hash = hex_sha256(b"hello world");
        assert_eq!(payload_hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");

        let authorization = client.sign(
            "PUT",
            "/backups/app-123/2024-01-15.zip",
            "my-bucket.s3.amazonaws.com",
            &payload_hash,
            "20240115T120000Z",
            "20240115",
        );

        assert_eq!(
            authorization,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20240115/us-east-1/s3/aws4_request, \
             SignedHeaders=host;x-amz-content-sha256;x-amz-date, \
             Signature=6aab9e8218af50a29c051857a9773c78bc185662fe1eaa3264e8b1031125ba0b"
        );
    }

    #[test]
    fn uri_encode_leaves_unreserved_characters_alone_and_percent_encodes_the_rest() {
        assert_eq!(uri_encode("abcXYZ019-._~"), "abcXYZ019-._~");
        assert_eq!(uri_encode("a b"), "a%20b");
        assert_eq!(uri_encode("a/b"), "a%2Fb");
        assert_eq!(uri_encode("2024-01-15T12:00:00Z"), "2024-01-15T12%3A00%3A00Z");
    }

    #[test]
    fn encode_key_path_leaves_the_separating_slashes_unencoded() {
        assert_eq!(encode_key_path("backups/app 123/2024:01.zip"), "backups/app%20123/2024%3A01.zip");
    }

    #[test]
    fn request_target_uses_virtual_hosted_style_by_default() {
        let client = S3Client::new(test_config(), "secret".to_string());
        let (host, path, url) = client.request_target("backups/a.zip").unwrap();
        assert_eq!(host, "my-bucket.s3.amazonaws.com");
        assert_eq!(path, "/backups/a.zip");
        assert_eq!(url, "https://my-bucket.s3.amazonaws.com/backups/a.zip");
    }

    #[test]
    fn request_target_uses_path_style_when_configured_for_minio() {
        let mut config = test_config();
        config.endpoint = "https://minio.example.internal:9000".to_string();
        config.path_style = true;
        let client = S3Client::new(config, "secret".to_string());
        let (host, path, url) = client.request_target("backups/a.zip").unwrap();
        assert_eq!(host, "minio.example.internal:9000");
        assert_eq!(path, "/my-bucket/backups/a.zip");
        assert_eq!(url, "https://minio.example.internal:9000/my-bucket/backups/a.zip");
    }

    #[test]
    fn prefixed_key_joins_a_configured_path_prefix() {
        let mut config = test_config();
        config.path_prefix = "/vibessh-backups/".to_string();
        let client = S3Client::new(config, "secret".to_string());
        assert_eq!(client.prefixed_key("app-1/backup.zip"), "vibessh-backups/app-1/backup.zip");
    }

    #[test]
    fn prefixed_key_is_a_no_op_when_unset() {
        let client = S3Client::new(test_config(), "secret".to_string());
        assert_eq!(client.prefixed_key("app-1/backup.zip"), "app-1/backup.zip");
    }

    #[test]
    fn request_target_rejects_an_endpoint_without_a_scheme() {
        let mut config = test_config();
        config.endpoint = "s3.amazonaws.com".to_string();
        let client = S3Client::new(config, "secret".to_string());
        assert!(client.request_target("key").is_err());
    }
    /// Backups carry application data and every request is signed with the
    /// destination's secret key - over plain HTTP both are in the clear.
    #[test]
    fn a_plaintext_http_endpoint_is_rejected() {
        let mut config = test_config();
        config.endpoint = "http://backups.example.com".to_string();
        let client = S3Client::new(config, "secret".to_string());
        let err = client.request_target("backups/a.zip").unwrap_err();
        assert!(err.to_string().contains("https"), "{err}");
    }

    /// The one legitimate exception: a MinIO instance on the same machine
    /// cannot put anything on a network.
    #[test]
    fn a_loopback_http_endpoint_is_allowed() {
        for endpoint in ["http://127.0.0.1:9000", "http://localhost:9000", "http://[::1]:9000"] {
            let mut config = test_config();
            config.endpoint = endpoint.to_string();
            config.path_style = true;
            let client = S3Client::new(config, "secret".to_string());
            assert!(client.request_target("backups/a.zip").is_ok(), "{endpoint} should be allowed");
        }
    }

    #[test]
    fn an_https_endpoint_is_always_allowed() {
        let mut config = test_config();
        config.endpoint = "https://backups.example.com".to_string();
        let client = S3Client::new(config, "secret".to_string());
        assert!(client.request_target("backups/a.zip").is_ok());
    }
}
