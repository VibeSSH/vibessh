use serde::{Deserialize, Serialize};

/// Where Application backups additionally get uploaded, on top of the
/// local `.vibessh-backups/` copy every backup already gets (see
/// `services::application_backup_service`'s own doc comment) - one global
/// destination for the whole VibeSSH install, not per-Application, same
/// "one thing to configure, every backup benefits" shape a single cloud
/// backend URL already has (`storage::cloud_config`). Non-secret; the
/// actual secret access key lives in the OS keyring (`storage::credentials::
/// store_backup_destination_secret`), never in this struct or the plain
/// JSON file it's persisted to.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupDestinationConfig {
    /// `false` (the default) means backups stay local-only, exactly
    /// today's behavior - configuring the rest of these fields alone
    /// doesn't start uploading anything until this is explicitly turned on.
    pub enabled: bool,
    /// Must include a scheme (`https://s3.amazonaws.com`,
    /// `https://<account>.r2.cloudflarestorage.com`,
    /// `https://minio.example.internal:9000`, ...).
    pub endpoint: String,
    /// AWS/R2 region code (e.g. `"us-east-1"`, `"auto"` for R2). MinIO
    /// accepts any non-empty string here - it doesn't enforce a real
    /// region, but SigV4 still needs one to compute the signing key.
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    /// Joined in front of every object key this app writes (e.g.
    /// `"vibessh-backups"`) - lets one bucket be safely shared with other
    /// tenants/uses. Empty means bucket root.
    pub path_prefix: String,
    /// `true` for `endpoint/bucket/key` addressing (what MinIO needs by
    /// default); `false` for `bucket.endpoint-host/key` (what AWS S3 and
    /// Cloudflare R2 both prefer).
    pub path_style: bool,
}

/// What `set_backup_destination` submits - mirrors `BackupDestinationConfig`
/// plus the write-only secret. `secret_access_key` blank means "leave the
/// currently stored secret alone," the same "the frontend never has the
/// real secret to resend" rule every other secret-bearing form in this app
/// already follows (see `services::application_service::
/// set_application_environment`'s own doc comment for the same pattern
/// applied to environment variable secrets).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBackupDestinationInput {
    pub enabled: bool,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub path_prefix: String,
    pub path_style: bool,
    pub secret_access_key: String,
}
