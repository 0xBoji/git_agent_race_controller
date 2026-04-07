use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::errors::GarcError;

pub const DEFAULT_SERVICE_TYPE: &str = "_camp._tcp.local.";
pub const DEFAULT_DISCOVERY_TIMEOUT_MS: u64 = 250;
pub const DEFAULT_CLAIM_SETTLE_MS: u64 = 150;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampConfig {
    pub agent: AgentConfig,
    #[serde(default)]
    pub discovery: DiscoveryConfig,
}

impl CampConfig {
    pub fn from_path(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow::Error::from(GarcError::MissingCampConfig {
                    path: path.display().to_string(),
                })
            } else {
                error.into()
            }
        })?;

        toml::from_str(&contents)
            .with_context(|| format!("failed to parse CAMP config `{}`", path.display()))
    }

    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        let parent = path.parent().filter(|value| !value.as_os_str().is_empty());
        if let Some(parent) = parent {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory `{}`", parent.display())
            })?;
        }

        let contents = toml::to_string_pretty(self).context("failed to serialize CAMP config")?;
        fs::write(path, format!("{contents}\n"))
            .with_context(|| format!("failed to write CAMP config `{}`", path.display()))
    }

    #[must_use]
    pub fn service_type(&self) -> &str {
        self.discovery
            .service_type
            .as_deref()
            .unwrap_or(DEFAULT_SERVICE_TYPE)
    }

    #[must_use]
    pub fn discovery_timeout_ms(&self) -> u64 {
        self.discovery
            .discovery_timeout_ms
            .unwrap_or(DEFAULT_DISCOVERY_TIMEOUT_MS)
    }

    #[must_use]
    pub fn mdns_port(&self) -> Option<u16> {
        self.discovery.mdns_port
    }

    #[must_use]
    pub fn claim_settle_ms(&self) -> u64 {
        self.discovery
            .claim_settle_ms
            .unwrap_or(DEFAULT_CLAIM_SETTLE_MS)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub project: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    #[serde(default)]
    pub service_type: Option<String>,
    #[serde(default)]
    pub mdns_port: Option<u16>,
    #[serde(default)]
    pub heartbeat_ms: Option<u64>,
    #[serde(default)]
    pub ttl_ms: Option<u64>,
    #[serde(default)]
    pub shared_secret_mode: Option<String>,
    #[serde(default)]
    pub discovery_timeout_ms: Option<u64>,
    #[serde(default)]
    pub claim_settle_ms: Option<u64>,
}

pub fn resolve_config_path(repo_root: &Path, config: &Path) -> PathBuf {
    if config.is_absolute() {
        config.to_path_buf()
    } else {
        repo_root.join(config)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Result;
    use tempfile::TempDir;

    use super::{CampConfig, DEFAULT_CLAIM_SETTLE_MS, DEFAULT_DISCOVERY_TIMEOUT_MS};

    #[test]
    fn config_parsing_preserves_optional_mdns_port() -> Result<()> {
        let tempdir = TempDir::new()?;
        let path = tempdir.path().join(".camp.toml");
        fs::write(
            &path,
            "[agent]\nid = \"local-agent\"\nproject = \"alpha\"\nbranch = \"main\"\n\n[discovery]\nservice_type = \"_camp._tcp.local.\"\nmdns_port = 54541\ndiscovery_timeout_ms = 900\n",
        )?;

        let config = CampConfig::from_path(&path)?;

        assert_eq!(config.mdns_port(), Some(54_541));
        assert_eq!(config.discovery_timeout_ms(), 900);
        Ok(())
    }

    #[test]
    fn config_parsing_preserves_optional_claim_settle_ms() -> Result<()> {
        let tempdir = TempDir::new()?;
        let path = tempdir.path().join(".camp.toml");
        fs::write(
            &path,
            "[agent]\nid = \"local-agent\"\nproject = \"alpha\"\nbranch = \"main\"\n\n[discovery]\nclaim_settle_ms = 375\n",
        )?;

        let config = CampConfig::from_path(&path)?;

        assert_eq!(config.claim_settle_ms(), 375);
        Ok(())
    }

    #[test]
    fn config_parsing_falls_back_to_default_discovery_timeout() -> Result<()> {
        let tempdir = TempDir::new()?;
        let path = tempdir.path().join(".camp.toml");
        fs::write(
            &path,
            "[agent]\nid = \"local-agent\"\nproject = \"alpha\"\nbranch = \"main\"\n",
        )?;

        let config = CampConfig::from_path(&path)?;

        assert_eq!(config.mdns_port(), None);
        assert_eq!(config.discovery_timeout_ms(), DEFAULT_DISCOVERY_TIMEOUT_MS);
        assert_eq!(config.claim_settle_ms(), DEFAULT_CLAIM_SETTLE_MS);
        Ok(())
    }
}
