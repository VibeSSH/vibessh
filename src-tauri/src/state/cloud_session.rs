//! In-memory cloud session (access token + cached profile) plus the
//! `CloudClient` configured for whatever backend URL is currently active.
//! The refresh token itself never lives here - it's always the OS keyring
//! (see storage::credentials) or nowhere; this only ever holds the
//! short-lived access token, which is fine to lose on every app restart
//! since `services::cloud_service::ensure_valid_access_token` transparently
//! re-derives it from the keyring-stored refresh token on first use.
use tokio::sync::Mutex;

use crate::cloud_client::CloudClient;
use crate::models::{CloudSessionInfo, CloudUserProfile};

pub struct CloudSession {
    pub access_token: String,
    /// Unix seconds.
    pub access_token_expires_at: i64,
    pub user: CloudUserProfile,
}

pub struct CloudStateInner {
    pub client: CloudClient,
    pub session: Option<CloudSession>,
}

pub struct CloudState {
    pub inner: Mutex<CloudStateInner>,
}

impl CloudState {
    pub fn new(backend_url: String) -> Self {
        Self { inner: Mutex::new(CloudStateInner { client: CloudClient::new(backend_url), session: None }) }
    }

    pub async fn session_info(&self) -> Option<CloudSessionInfo> {
        let inner = self.inner.lock().await;
        inner.session.as_ref().map(|session| CloudSessionInfo { user: session.user.clone() })
    }

    pub async fn backend_url(&self) -> String {
        self.inner.lock().await.client.base_url().to_string()
    }

    /// Changing the backend URL means "you're now pointed at a possibly
    /// different account entirely" - the in-memory session and any stored
    /// refresh token for the *old* backend are cleared rather than silently
    /// reused against a new one.
    pub async fn set_backend_url(&self, backend_url: String) {
        let mut inner = self.inner.lock().await;
        inner.client = CloudClient::new(backend_url);
        inner.session = None;
    }
}
