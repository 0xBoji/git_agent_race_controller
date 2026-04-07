use anyhow::Result;
use serde::Serialize;

use crate::mesh::MeshPeer;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckoutStatus {
    CheckedOut,
    Diverted,
    Forced,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionBasis {
    MeshClear,
    BranchOccupied,
    ClaimArbitrationWon,
    ClaimArbitrationLost,
    ForceBypass,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckoutOutput {
    pub status: CheckoutStatus,
    pub requested_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occupied_by: Option<String>,
    pub decision_basis: DecisionBasis,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub observed_claims: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub observed_peers: Vec<ObservedPeerOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_winner: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub decision_trace: Vec<String>,
    pub actual_branch: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InitOutput {
    pub status: &'static str,
    pub agent_id: String,
    pub project: String,
    pub hook_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusOutput {
    pub status: &'static str,
    pub agent_id: String,
    pub project: String,
    pub local_branch: String,
    pub peers: Vec<MeshPeer>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub occupied_branches: Vec<OccupiedBranchSummary>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub active_claims: Vec<ActiveClaimSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OccupiedBranchSummary {
    pub branch: String,
    pub occupied_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveClaimSummary {
    pub branch: String,
    pub claimants: Vec<String>,
    pub claim_winner: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObservedPeerOutput {
    pub agent_id: String,
    pub current_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorOutput {
    pub status: &'static str,
    pub message: String,
}

pub fn print_checkout(output: &CheckoutOutput, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(output)?);
    } else {
        let decision_basis = serde_json::to_string(&output.decision_basis)?;
        println!("{}", output.message);
        println!("requested branch: {}", output.requested_branch);
        println!("decision basis: {}", decision_basis.trim_matches('"'));
        println!("actual branch: {}", output.actual_branch);
        if let Some(occupied_by) = &output.occupied_by {
            println!("occupied by: {occupied_by}");
        }
        if !output.observed_claims.is_empty() {
            println!("observed claims: {}", output.observed_claims.join(", "));
        }
        if !output.observed_peers.is_empty() {
            println!("observed peers:");
            for peer in &output.observed_peers {
                if let Some(intent_branch) = &peer.intent_branch {
                    println!(
                        "- {} on {} claiming {}",
                        peer.agent_id, peer.current_branch, intent_branch
                    );
                } else {
                    println!("- {} on {}", peer.agent_id, peer.current_branch);
                }
            }
        }
        if let Some(claim_winner) = &output.claim_winner {
            println!("claim winner: {claim_winner}");
        }
        if !output.decision_trace.is_empty() {
            println!("decision trace:");
            for step in &output.decision_trace {
                println!("- {step}");
            }
        }
    }

    Ok(())
}

pub fn print_init(output: &InitOutput, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(output)?);
    } else {
        println!("{}", output.message);
        println!("agent id: {}", output.agent_id);
        println!("project: {}", output.project);
        println!("hook: {}", output.hook_path);
    }

    Ok(())
}

pub fn print_status(output: &StatusOutput, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(output)?);
    } else {
        println!("project: {}", output.project);
        println!("local agent: {}", output.agent_id);
        println!("local branch: {}", output.local_branch);
        if output.peers.is_empty() {
            println!("mesh peers: none discovered");
        } else {
            println!("mesh peers:");
            for peer in &output.peers {
                if let Some(intent_branch) = &peer.intent_branch {
                    println!(
                        "- {} on {} ({}) claiming {}",
                        peer.agent_id, peer.current_branch, peer.current_project, intent_branch
                    );
                } else {
                    println!(
                        "- {} on {} ({})",
                        peer.agent_id, peer.current_branch, peer.current_project
                    );
                }
            }
        }
        if !output.occupied_branches.is_empty() {
            println!("occupied branches:");
            for branch in &output.occupied_branches {
                println!("- {}: {}", branch.branch, branch.occupied_by.join(", "));
            }
        }
        if !output.active_claims.is_empty() {
            println!("active claims:");
            for claim in &output.active_claims {
                println!(
                    "- {}: {} (winner: {})",
                    claim.branch,
                    claim.claimants.join(", "),
                    claim.claim_winner
                );
            }
        }
    }

    Ok(())
}

pub fn print_error(message: String) -> Result<()> {
    let output = ErrorOutput {
        status: "error",
        message,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CheckoutOutput, CheckoutStatus, DecisionBasis, ObservedPeerOutput};

    #[test]
    fn checkout_output_serializes_to_expected_shape() {
        let output = CheckoutOutput {
            status: CheckoutStatus::Diverted,
            requested_branch: "feature-login".to_owned(),
            occupied_by: Some("qa-agent-01".to_owned()),
            decision_basis: DecisionBasis::ClaimArbitrationLost,
            observed_claims: vec!["qa-agent-01".to_owned()],
            observed_peers: vec![ObservedPeerOutput {
                agent_id: "qa-agent-01".to_owned(),
                current_branch: "main".to_owned(),
                intent_branch: Some("feature-login".to_owned()),
            }],
            claim_winner: Some("qa-agent-01".to_owned()),
            decision_trace: vec![
                "published_claim".to_owned(),
                "observed_claimants:qa-agent-01".to_owned(),
                "decision:diverted".to_owned(),
            ],
            actual_branch: "feature-login--coder-01".to_owned(),
            message: "Target branch is currently locked. Checked out sub-branch to prevent race conditions.".to_owned(),
        };

        let json = serde_json::to_string_pretty(&output).expect("checkout output should serialize");
        assert!(json.contains("\"status\": \"diverted\""));
        assert!(json.contains("\"requested_branch\": \"feature-login\""));
        assert!(json.contains("\"occupied_by\": \"qa-agent-01\""));
        assert!(json.contains("\"decision_basis\": \"claim_arbitration_lost\""));
        assert!(json.contains("\"observed_claims\": ["));
        assert!(json.contains("\"observed_peers\": ["));
        assert!(json.contains("\"claim_winner\": \"qa-agent-01\""));
        assert!(json.contains("\"decision_trace\": ["));
        assert!(json.contains("\"actual_branch\": \"feature-login--coder-01\""));
    }
}
