use crate::mesh::MeshPeer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollisionResult {
    Clear,
    Occupied { by: String },
}

#[must_use]
pub fn active_branch_occupier(
    peers: &[MeshPeer],
    project: &str,
    branch: &str,
    local_agent_id: &str,
) -> Option<String> {
    peers
        .iter()
        .filter(|peer| {
            peer.current_project == project
                && peer.current_branch == branch
                && peer.agent_id != local_agent_id
        })
        .map(|peer| peer.agent_id.clone())
        .min()
}

#[must_use]
pub fn observed_claimants(
    peers: &[MeshPeer],
    project: &str,
    branch: &str,
    local_agent_id: &str,
) -> Vec<String> {
    let mut claimants = peers
        .iter()
        .filter(|peer| {
            peer.current_project == project
                && peer.intent_branch.as_deref() == Some(branch)
                && peer.agent_id != local_agent_id
        })
        .map(|peer| peer.agent_id.clone())
        .collect::<Vec<_>>();
    claimants.sort();
    claimants.dedup();
    claimants
}

#[must_use]
pub fn claim_winner(
    peers: &[MeshPeer],
    project: &str,
    branch: &str,
    local_agent_id: &str,
) -> Option<String> {
    observed_claimants(peers, project, branch, local_agent_id)
        .into_iter()
        .chain(std::iter::once(local_agent_id.to_owned()))
        .min()
}

#[must_use]
pub fn detect_collision(
    peers: &[MeshPeer],
    project: &str,
    branch: &str,
    local_agent_id: &str,
) -> CollisionResult {
    if let Some(by) = active_branch_occupier(peers, project, branch, local_agent_id) {
        return CollisionResult::Occupied { by };
    }

    let winner = claim_winner(peers, project, branch, local_agent_id);

    match winner.as_deref() {
        Some(agent_id) if agent_id != local_agent_id => CollisionResult::Occupied {
            by: agent_id.to_owned(),
        },
        _ => CollisionResult::Clear,
    }
}

#[must_use]
pub fn diverted_branch_name(requested_branch: &str, local_agent_id: &str) -> String {
    // The diversion format intentionally keeps the requested branch visible and appends a
    // sanitized agent suffix. That makes it obvious which upstream workstream the sub-branch
    // belongs to while still avoiding invalid Git ref characters from arbitrary agent IDs.
    format!(
        "{requested_branch}--{}",
        sanitize_branch_component(local_agent_id)
    )
}

#[must_use]
pub fn sanitize_branch_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    let mut previous_was_dash = false;

    for character in value.chars() {
        let allowed = character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.');
        if allowed {
            sanitized.push(character.to_ascii_lowercase());
            previous_was_dash = false;
        } else if !previous_was_dash {
            sanitized.push('-');
            previous_was_dash = true;
        }
    }

    sanitized.truncate(sanitized.trim_end_matches('-').len());
    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "agent".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CollisionResult, detect_collision, diverted_branch_name, sanitize_branch_component,
    };
    use crate::mesh::MeshPeer;

    fn peer(agent_id: &str, project: &str, branch: &str) -> MeshPeer {
        MeshPeer {
            agent_id: agent_id.to_owned(),
            current_project: project.to_owned(),
            current_branch: branch.to_owned(),
            intent_branch: None,
            fullname: format!("{agent_id}._camp._tcp.local."),
            port: 7000,
        }
    }

    #[test]
    fn collision_detection_ignores_other_projects_and_self() {
        let peers = vec![
            peer("agent-self", "alpha", "feature-login"),
            peer("agent-other-project", "beta", "feature-login"),
            peer("agent-remote", "alpha", "feature-login"),
        ];

        let result = detect_collision(&peers, "alpha", "feature-login", "agent-self");
        assert_eq!(
            result,
            CollisionResult::Occupied {
                by: "agent-remote".to_owned()
            }
        );
    }

    #[test]
    fn branch_suffix_is_sanitized_for_git_ref_safety() {
        assert_eq!(sanitize_branch_component("Coder 01/QA"), "coder-01-qa");
        assert_eq!(
            diverted_branch_name("feature-login", "Coder 01/QA"),
            "feature-login--coder-01-qa"
        );
    }

    #[test]
    fn lower_lexicographic_claimant_wins_branch_arbitration() {
        let peers = vec![MeshPeer {
            agent_id: "alpha-agent".to_owned(),
            current_project: "alpha".to_owned(),
            current_branch: "main".to_owned(),
            intent_branch: Some("feature-login".to_owned()),
            fullname: "garc-claim-alpha-agent._camp._tcp.local.".to_owned(),
            port: 7000,
        }];

        let result = detect_collision(&peers, "alpha", "feature-login", "local-agent");
        assert_eq!(
            result,
            CollisionResult::Occupied {
                by: "alpha-agent".to_owned()
            }
        );
    }

    #[test]
    fn intent_claims_from_other_projects_are_ignored() {
        let peers = vec![MeshPeer {
            agent_id: "alpha-agent".to_owned(),
            current_project: "beta".to_owned(),
            current_branch: "main".to_owned(),
            intent_branch: Some("feature-login".to_owned()),
            fullname: "garc-claim-alpha-agent._camp._tcp.local.".to_owned(),
            port: 7000,
        }];

        let result = detect_collision(&peers, "alpha", "feature-login", "local-agent");
        assert_eq!(result, CollisionResult::Clear);
    }

    #[test]
    fn active_branch_occupancy_beats_claim_arbitration() {
        let peers = vec![
            MeshPeer {
                agent_id: "zeta-agent".to_owned(),
                current_project: "alpha".to_owned(),
                current_branch: "feature-login".to_owned(),
                intent_branch: None,
                fullname: "zeta-agent._camp._tcp.local.".to_owned(),
                port: 7000,
            },
            MeshPeer {
                agent_id: "alpha-agent".to_owned(),
                current_project: "alpha".to_owned(),
                current_branch: "main".to_owned(),
                intent_branch: Some("feature-login".to_owned()),
                fullname: "garc-claim-alpha-agent._camp._tcp.local.".to_owned(),
                port: 7000,
            },
        ];

        let result = detect_collision(&peers, "alpha", "feature-login", "local-agent");
        assert_eq!(
            result,
            CollisionResult::Occupied {
                by: "zeta-agent".to_owned()
            }
        );
    }
}
