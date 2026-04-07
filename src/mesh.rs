use std::{
    collections::BTreeMap,
    env, fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo, UnregisterStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{config::CampConfig, errors::GarcError};

const AGENT_ID: &str = "agent_id";
const CURRENT_BRANCH: &str = "current_branch";
const CURRENT_PROJECT: &str = "current_project";
const INTENT_BRANCH: &str = "intent_branch";
const CLAIM_PORT_FALLBACK: u16 = 7000;
const CLAIM_STATE_FILE_NAME: &str = "claim-state.json";
const LAST_TRACE_FILE_NAME: &str = "last-checkout-trace.json";
const TRACE_HISTORY_DIR_NAME: &str = "trace-history";
const TRACE_HISTORY_LIMIT: usize = 10;
const CLAIM_DISCOVERY_RETRIES: usize = 3;
const DEFAULT_CAMP_REST_URL: &str = "http://127.0.0.1:9999/agents";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshPeer {
    pub agent_id: String,
    pub current_branch: String,
    pub current_project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_branch: Option<String>,
    pub fullname: String,
    pub port: u16,
}

impl MeshPeer {
    fn from_resolved_service(service: &ResolvedService) -> Result<Self> {
        let fullname = service.get_fullname().to_owned();

        let agent_id = required_property(service, &fullname, AGENT_ID)?;
        let current_branch = required_property(service, &fullname, CURRENT_BRANCH)?;
        let current_project = required_property(service, &fullname, CURRENT_PROJECT)?;

        Ok(Self {
            agent_id,
            current_branch,
            current_project,
            intent_branch: optional_property(service, INTENT_BRANCH),
            fullname,
            port: service.get_port(),
        })
    }
}

pub struct ClaimHandle {
    daemon: Option<ServiceDaemon>,
    fullname: Option<String>,
    settle_ms: Option<u64>,
    claim_state_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalClaimState {
    pub agent_id: String,
    pub current_project: String,
    pub current_branch: String,
    pub intent_branch: String,
}

impl ClaimHandle {
    #[must_use]
    pub fn settle_required(&self) -> bool {
        self.settle_ms.is_some()
    }

    pub fn settle(&self) {
        if let Some(settle_ms) = self.settle_ms {
            thread::sleep(Duration::from_millis(settle_ms));
        }
    }
}

impl Drop for ClaimHandle {
    fn drop(&mut self) {
        if let Some(path) = &self.claim_state_path {
            let _ = fs::remove_file(path);
        }

        if let (Some(daemon), Some(fullname)) = (&self.daemon, &self.fullname)
            && let Ok(receiver) = daemon.unregister(fullname)
        {
            let _ = receiver
                .recv_timeout(Duration::from_millis(100))
                .map(|status| matches!(status, UnregisterStatus::OK | UnregisterStatus::NotFound));
        }

        if let Some(daemon) = &self.daemon {
            let _ = daemon.shutdown();
        }
    }
}

pub fn publish_branch_claim(
    config: &CampConfig,
    git_dir: &Path,
    branch: &str,
    claim_settle_ms: u64,
) -> Result<ClaimHandle> {
    if env::var_os("GARC_MESH_SNAPSHOT_JSON").is_some() {
        return Ok(ClaimHandle {
            daemon: None,
            fullname: None,
            settle_ms: None,
            claim_state_path: None,
        });
    }

    let daemon = if let Some(mdns_port) = config.mdns_port() {
        ServiceDaemon::new_with_port(mdns_port)
            .with_context(|| format!("failed to start mDNS claim daemon on port `{mdns_port}`"))?
    } else {
        ServiceDaemon::new().context("failed to start mDNS claim daemon")?
    };

    // The claim record uses its own service instance name so it can coexist with the steady-state
    // CAMP announcement, but the canonical agent identity still lives in TXT metadata.
    let instance_name = format!("garc-claim-{}", sanitize_dns_label(&config.agent.id));
    let host_name = format!("{instance_name}.local.");
    let properties = [
        (AGENT_ID, config.agent.id.as_str()),
        (CURRENT_PROJECT, config.agent.project.as_str()),
        (CURRENT_BRANCH, config.agent.branch.as_str()),
        (INTENT_BRANCH, branch),
    ];
    let service = ServiceInfo::new(
        config.service_type(),
        &instance_name,
        &host_name,
        "",
        config.agent.port.unwrap_or(CLAIM_PORT_FALLBACK),
        &properties[..],
    )
    .context("failed to construct mDNS branch-claim service")?
    .enable_addr_auto();
    let fullname = service.get_fullname().to_owned();

    daemon
        .register(service)
        .context("failed to publish temporary branch-claim service")?;

    let claim_state = LocalClaimState {
        agent_id: config.agent.id.clone(),
        current_project: config.agent.project.clone(),
        current_branch: config.agent.branch.clone(),
        intent_branch: branch.to_owned(),
    };
    let claim_state_path = write_local_claim_state(git_dir, &claim_state)?;

    Ok(ClaimHandle {
        daemon: Some(daemon),
        fullname: Some(fullname),
        settle_ms: Some(claim_settle_ms),
        claim_state_path: Some(claim_state_path),
    })
}

pub fn discover_peers_with_retry(config: &CampConfig) -> Result<Vec<MeshPeer>> {
    discover_peers_with_retry_metadata(config).map(|(peers, _)| peers)
}

pub fn discover_peers_with_retry_metadata(config: &CampConfig) -> Result<(Vec<MeshPeer>, usize)> {
    let mut last_error = None;

    for attempt in 0..CLAIM_DISCOVERY_RETRIES {
        match discover_peers(config) {
            Ok(peers) => return Ok((peers, attempt + 1)),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < CLAIM_DISCOVERY_RETRIES {
                    thread::sleep(Duration::from_millis(retry_backoff_ms(attempt)));
                }
            }
        }
    }

    Err(last_error.expect("retry loop should record at least one discovery error"))
}

pub fn discover_peers(config: &CampConfig) -> Result<Vec<MeshPeer>> {
    if let Some(snapshot) = env::var_os("GARC_MESH_SNAPSHOT_JSON") {
        let peers = serde_json::from_str(snapshot.to_string_lossy().as_ref())
            .context("failed to parse GARC_MESH_SNAPSHOT_JSON")?;
        return Ok(peers);
    }

    if let Some(peers) = discover_peers_via_rest(config)? {
        return Ok(peers);
    }

    let service_type = config.service_type().to_owned();
    let timeout = Duration::from_millis(config.discovery_timeout_ms());
    let daemon = if let Some(mdns_port) = config.mdns_port() {
        ServiceDaemon::new_with_port(mdns_port).with_context(|| {
            format!("failed to start mDNS discovery daemon on port `{mdns_port}`")
        })?
    } else {
        ServiceDaemon::new().context("failed to start mDNS discovery daemon")?
    };
    let receiver = daemon
        .browse(&service_type)
        .with_context(|| format!("failed to browse mDNS service type `{service_type}`"))?;

    let mut peers = BTreeMap::<String, MeshPeer>::new();
    let deadline = Instant::now() + timeout;

    // CAMP relies on mDNS TTL expiry to evict crashed peers. `garc` intentionally trusts the
    // current browse snapshot instead of layering an extra lock store on top, so orphaned locks
    // disappear as soon as the underlying CAMP announcements age out of the mesh.
    while Instant::now() < deadline {
        match receiver.try_recv() {
            Ok(ServiceEvent::ServiceResolved(service)) => {
                if let Ok(peer) = MeshPeer::from_resolved_service(&service) {
                    peers.insert(peer.fullname.clone(), peer);
                }
            }
            Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                peers.remove(&fullname);
            }
            Ok(_) => {}
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }

    let _ = daemon.stop_browse(&service_type);
    let _ = daemon.shutdown();

    Ok(peers.into_values().collect())
}

fn discover_peers_via_rest(config: &CampConfig) -> Result<Option<Vec<MeshPeer>>> {
    if let Some(snapshot) = env::var_os("GARC_CAMP_REST_JSON") {
        let agents: Vec<RestAgentRecord> =
            serde_json::from_str(snapshot.to_string_lossy().as_ref())
                .context("failed to parse GARC_CAMP_REST_JSON")?;
        return Ok(Some(
            agents
                .into_iter()
                .map(|agent| MeshPeer {
                    agent_id: agent.id,
                    current_branch: agent.branch,
                    current_project: agent.project,
                    intent_branch: agent.metadata.get(INTENT_BRANCH).cloned(),
                    fullname: agent.instance_name,
                    port: agent.port,
                })
                .collect(),
        ));
    }

    let base_url = env::var("GARC_CAMP_REST_URL").unwrap_or_else(|_| {
        config
            .camp_rest_url()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{DEFAULT_CAMP_REST_URL}?project={}", config.agent.project))
    });
    let response = match http_get_json(&base_url) {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(None),
        Err(_) => return Ok(None),
    };

    let agents: Vec<RestAgentRecord> =
        serde_json::from_value(response).context("failed to parse camp REST agent list")?;
    Ok(Some(
        agents
            .into_iter()
            .map(|agent| MeshPeer {
                agent_id: agent.id,
                current_branch: agent.branch,
                current_project: agent.project,
                intent_branch: agent.metadata.get(INTENT_BRANCH).cloned(),
                fullname: agent.instance_name,
                port: agent.port,
            })
            .collect(),
    ))
}

pub fn update_local_branch(
    config_path: &std::path::Path,
    config: &mut CampConfig,
    branch: &str,
) -> Result<()> {
    config.agent.branch = branch.to_owned();
    config.save_to_path(config_path)
}

pub fn read_local_claim_state(git_dir: &Path) -> Result<Option<LocalClaimState>> {
    let path = claim_state_path(git_dir);
    match fs::read_to_string(&path) {
        Ok(contents) => {
            let claim = serde_json::from_str(&contents).with_context(|| {
                format!("failed to parse local claim state `{}`", path.display())
            })?;
            Ok(Some(claim))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("failed to read local claim state `{}`", path.display())),
    }
}

pub fn read_last_checkout_trace(git_dir: &Path) -> Result<Option<Value>> {
    let path = garc_state_dir(git_dir).join(LAST_TRACE_FILE_NAME);
    read_json_file_if_exists(&path)
}

pub fn read_trace_history(git_dir: &Path) -> Result<Vec<Value>> {
    let history_dir = trace_history_dir(git_dir);
    let mut paths = match fs::read_dir(&history_dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(|file_type| file_type.is_file())
                    .map(|_| entry.path())
            })
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to read trace history directory `{}`",
                    history_dir.display()
                )
            });
        }
    };
    paths.sort();
    paths.reverse();

    paths
        .into_iter()
        .map(|path| {
            let contents = fs::read_to_string(&path).with_context(|| {
                format!("failed to read trace history entry `{}`", path.display())
            })?;
            serde_json::from_str(&contents).with_context(|| {
                format!("failed to parse trace history entry `{}`", path.display())
            })
        })
        .collect()
}

pub fn write_last_checkout_trace(git_dir: &Path, trace: &impl Serialize) -> Result<PathBuf> {
    // This file is intentionally overwrite-only and local to one repository clone. It exists to
    // help operators inspect the most recent checkout arbitration, not to coordinate future ones.
    let path = garc_state_dir(git_dir).join(LAST_TRACE_FILE_NAME);
    fs::create_dir_all(garc_state_dir(git_dir)).with_context(|| {
        format!(
            "failed to create garc state directory `{}`",
            garc_state_dir(git_dir).display()
        )
    })?;
    let contents =
        serde_json::to_string_pretty(trace).context("failed to serialize last checkout trace")?;
    fs::write(&path, format!("{contents}\n"))
        .with_context(|| format!("failed to write last checkout trace `{}`", path.display()))?;
    let history_dir = trace_history_dir(git_dir);
    fs::create_dir_all(&history_dir).with_context(|| {
        format!(
            "failed to create trace history directory `{}`",
            history_dir.display()
        )
    })?;
    let history_entry_path = history_dir.join(next_trace_history_file_name()?);
    fs::write(&history_entry_path, format!("{contents}\n")).with_context(|| {
        format!(
            "failed to write trace history entry `{}`",
            history_entry_path.display()
        )
    })?;
    prune_trace_history(&history_dir)?;
    Ok(path)
}

fn required_property(
    service: &ResolvedService,
    fullname: &str,
    field: &'static str,
) -> Result<String> {
    let property = service
        .get_properties()
        .iter()
        .find(|property| property.key() == field)
        .ok_or_else(|| GarcError::MissingTxtField {
            fullname: fullname.to_owned(),
            field,
        })?;

    let value = property.val().ok_or_else(|| GarcError::MissingTxtField {
        fullname: fullname.to_owned(),
        field,
    })?;

    String::from_utf8(value.to_vec()).map_err(|_| {
        GarcError::InvalidTxtFieldEncoding {
            fullname: fullname.to_owned(),
            field,
        }
        .into()
    })
}

fn optional_property(service: &ResolvedService, field: &'static str) -> Option<String> {
    service
        .get_properties()
        .iter()
        .find(|property| property.key() == field)
        .and_then(|property| property.val())
        .and_then(|value| String::from_utf8(value.to_vec()).ok())
}

fn sanitize_dns_label(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "agent".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn write_local_claim_state(git_dir: &Path, claim_state: &LocalClaimState) -> Result<PathBuf> {
    let path = claim_state_path(git_dir);
    fs::create_dir_all(garc_state_dir(git_dir)).with_context(|| {
        format!(
            "failed to create claim state directory `{}`",
            garc_state_dir(git_dir).display()
        )
    })?;

    let contents = serde_json::to_string_pretty(claim_state)
        .context("failed to serialize local claim state")?;
    fs::write(&path, format!("{contents}\n"))
        .with_context(|| format!("failed to write local claim state `{}`", path.display()))?;
    Ok(path)
}

fn claim_state_path(git_dir: &Path) -> PathBuf {
    garc_state_dir(git_dir).join(CLAIM_STATE_FILE_NAME)
}

fn garc_state_dir(git_dir: &Path) -> PathBuf {
    git_dir.join("garc")
}

fn trace_history_dir(git_dir: &Path) -> PathBuf {
    garc_state_dir(git_dir).join(TRACE_HISTORY_DIR_NAME)
}

fn next_trace_history_file_name() -> Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")?;
    Ok(format!("{:020}.json", now.as_nanos()))
}

fn prune_trace_history(history_dir: &Path) -> Result<()> {
    let mut entries = fs::read_dir(history_dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_file())
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();
    entries.sort();

    let excess = entries.len().saturating_sub(TRACE_HISTORY_LIMIT);
    for path in entries.into_iter().take(excess) {
        fs::remove_file(&path)
            .with_context(|| format!("failed to prune trace history entry `{}`", path.display()))?;
    }

    Ok(())
}

pub fn retry_backoff_ms(attempt: usize) -> u64 {
    // The backoff stays intentionally tiny and capped. We only want enough breathing room for
    // LAN discovery jitter to settle, not a long retry ladder that makes contested checkouts
    // feel hung from an operator's perspective.
    match attempt {
        0 => 25,
        1 => 50,
        _ => 100,
    }
}

fn read_json_file_if_exists(path: &Path) -> Result<Option<Value>> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let value = serde_json::from_str(&contents)
                .with_context(|| format!("failed to parse JSON file `{}`", path.display()))?;
            Ok(Some(value))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read JSON file `{}`", path.display()))
        }
    }
}

fn http_get_json(url: &str) -> Result<Option<Value>> {
    let without_scheme = match url.strip_prefix("http://") {
        Some(value) => value,
        None => return Ok(None),
    };
    let (host_port, path) = match without_scheme.split_once('/') {
        Some((host_port, path)) => (host_port, format!("/{}", path)),
        None => (without_scheme, "/".to_owned()),
    };
    let (host, port) = match host_port.split_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().unwrap_or(80)),
        None => (host_port, 80),
    };

    let mut stream = match TcpStream::connect((host, port)) {
        Ok(stream) => stream,
        Err(_) => return Ok(None),
    };
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .context("failed to write REST request")?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .context("failed to read REST response")?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .context("REST response missing header separator")?;
    if !headers.starts_with("HTTP/1.1 200") && !headers.starts_with("HTTP/1.0 200") {
        return Ok(None);
    }

    let value = serde_json::from_str(body).context("failed to parse REST JSON body")?;
    Ok(Some(value))
}

#[derive(Debug, Deserialize)]
struct RestAgentRecord {
    id: String,
    instance_name: String,
    project: String,
    branch: String,
    port: u16,
    #[serde(default)]
    metadata: std::collections::BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Result;
    use tempfile::TempDir;

    use super::{retry_backoff_ms, write_last_checkout_trace};

    #[test]
    fn retry_backoff_grows_without_exploding() {
        assert_eq!(retry_backoff_ms(0), 25);
        assert_eq!(retry_backoff_ms(1), 50);
        assert_eq!(retry_backoff_ms(2), 100);
        assert_eq!(retry_backoff_ms(3), 100);
    }

    #[test]
    fn persisted_trace_history_is_bounded() -> Result<()> {
        let tempdir = TempDir::new()?;
        let git_dir = tempdir.path().join(".git");
        fs::create_dir_all(&git_dir)?;

        for index in 0..12 {
            let trace = serde_json::json!({
                "status": "checked_out",
                "requested_branch": format!("feature-{index}")
            });
            write_last_checkout_trace(&git_dir, &trace)?;
        }

        let history_dir = git_dir.join("garc/trace-history");
        let history_entries = fs::read_dir(&history_dir)?.count();
        assert_eq!(history_entries, 10);
        assert!(git_dir.join("garc/last-checkout-trace.json").exists());
        Ok(())
    }
}
