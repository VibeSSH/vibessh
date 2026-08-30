use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::AgentResult;

const IDENTITY_FILE_NAME: &str = "identity.json";

/// The agent's durable identity. Generated once on first run and reused on
/// every restart, so pairing (Etap E) and the desktop's server list can
/// recognize "the same agent" across reboots/upgrades.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdentity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
}

impl AgentIdentity {
    pub fn load_or_create(data_dir: &Path) -> AgentResult<Self> {
        let path = identity_path(data_dir);

        if path.exists() {
            let bytes = std::fs::read(&path)?;
            return Ok(serde_json::from_slice(&bytes)?);
        }

        let identity = Self {
            id: Uuid::new_v4(),
            created_at: Utc::now(),
        };
        std::fs::create_dir_all(data_dir)?;
        std::fs::write(&path, serde_json::to_vec_pretty(&identity)?)?;
        Ok(identity)
    }
}

fn identity_path(data_dir: &Path) -> PathBuf {
    data_dir.join(IDENTITY_FILE_NAME)
}
