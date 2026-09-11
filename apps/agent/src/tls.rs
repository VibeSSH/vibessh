//! Self-signed TLS for the public WebSocket endpoint (Etap K security
//! review). Before this, the pairing code and the durable credential
//! issued on a successful pairing both went out in plaintext - trivially
//! readable by anyone who could observe the traffic (a shared network, a
//! malicious router, an ISP). This defeats that: passive eavesdropping on
//! the connection no longer works.
//!
//! What it does NOT defend against: an ACTIVE attacker sitting on the very
//! first connection, presenting their own certificate before the desktop
//! has anything to compare it to. There's no side channel today to seed a
//! pin ahead of time (the pairing code is generated blind, before the
//! desktop has ever talked to the agent) - the same fundamental bootstrap
//! problem SSH has before a host's first `known_hosts` entry. Pinning this
//! certificate's fingerprint after the first successful connection, and
//! rejecting a mismatch on every connection after that, is the concrete
//! next hardening step - not implemented here, but the certificate is
//! already persisted (not regenerated per boot) specifically so a future
//! pin stays valid across restarts.

use std::path::{Path, PathBuf};
use std::sync::Once;

use crate::errors::{AgentError, AgentResult};

const CERT_FILE_NAME: &str = "tls_cert.pem";
const KEY_FILE_NAME: &str = "tls_key.pem";

static CRYPTO_PROVIDER_INSTALLED: Once = Once::new();

/// rustls 0.23 needs a process-wide default crypto backend selected before
/// any `RustlsConfig`/`ClientConfig` is built - with more than one backend
/// reachable transitively (ring vs aws-lc-rs, pulled in by whichever of our
/// dependencies happens to enable which), it refuses to guess. `Once`
/// because this runs once per test binary too, where `load_or_create` (and
/// so this) is called once per `#[tokio::test]`, all in the same process.
fn ensure_crypto_provider_installed() {
    CRYPTO_PROVIDER_INSTALLED.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub struct TlsPaths {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// Loads the persisted cert/key if this agent already has one, otherwise
/// generates and persists a new self-signed pair. Persisting (not
/// regenerating per boot) matters even before pinning exists: a
/// certificate that changes every restart would make a future
/// pin-on-first-use scheme worthless.
pub fn load_or_create(data_dir: &Path) -> AgentResult<TlsPaths> {
    ensure_crypto_provider_installed();

    let cert_path = data_dir.join(CERT_FILE_NAME);
    let key_path = data_dir.join(KEY_FILE_NAME);

    if cert_path.exists() && key_path.exists() {
        return Ok(TlsPaths { cert_path, key_path });
    }

    let rcgen::CertifiedKey { cert, key_pair } = rcgen::generate_simple_self_signed(vec!["vibessh-agent".to_string()])
        .map_err(|err| AgentError::Config(format!("failed to generate a TLS certificate: {err}")))?;

    std::fs::create_dir_all(data_dir)?;
    std::fs::write(&cert_path, cert.pem())?;
    std::fs::write(&key_path, key_pair.serialize_pem())?;

    Ok(TlsPaths { cert_path, key_path })
}
