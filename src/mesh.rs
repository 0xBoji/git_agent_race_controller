use std::{
    collections::BTreeMap,
    env, thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent};
use serde::{Deserialize, Serialize};

use crate::{config::CampConfig, errors::GarcError};

const AGENT_ID: &str = "agent_id";
const CURRENT_BRANCH: &str = "current_branch";
const CURRENT_PROJECT: &str = "current_project";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshPeer {
    pub agent_id: String,
    pub current_branch: String,
    pub current_project: String,
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
            fullname,
            port: service.get_port(),
        })
    }
}

pub fn discover_peers(config: &CampConfig) -> Result<Vec<MeshPeer>> {
    if let Some(snapshot) = env::var_os("GARC_MESH_SNAPSHOT_JSON") {
        let peers = serde_json::from_str(snapshot.to_string_lossy().as_ref())
            .context("failed to parse GARC_MESH_SNAPSHOT_JSON")?;
        return Ok(peers);
    }

    let service_type = config.service_type().to_owned();
    let timeout = Duration::from_millis(config.discovery_timeout_ms());
    let daemon = ServiceDaemon::new().context("failed to start mDNS discovery daemon")?;
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

pub fn update_local_branch(
    config_path: &std::path::Path,
    config: &mut CampConfig,
    branch: &str,
) -> Result<()> {
    config.agent.branch = branch.to_owned();
    config.save_to_path(config_path)
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
