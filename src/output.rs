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
pub struct CheckoutOutput {
    pub status: CheckoutStatus,
    pub requested_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occupied_by: Option<String>,
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
        println!("{}", output.message);
        println!("requested branch: {}", output.requested_branch);
        println!("actual branch: {}", output.actual_branch);
        if let Some(occupied_by) = &output.occupied_by {
            println!("occupied by: {occupied_by}");
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
                println!(
                    "- {} on {} ({})",
                    peer.agent_id, peer.current_branch, peer.current_project
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
    use super::{CheckoutOutput, CheckoutStatus};

    #[test]
    fn checkout_output_serializes_to_expected_shape() {
        let output = CheckoutOutput {
            status: CheckoutStatus::Diverted,
            requested_branch: "feature-login".to_owned(),
            occupied_by: Some("qa-agent-01".to_owned()),
            actual_branch: "feature-login--coder-01".to_owned(),
            message: "Target branch is currently locked. Checked out sub-branch to prevent race conditions.".to_owned(),
        };

        let json = serde_json::to_string_pretty(&output).expect("checkout output should serialize");
        assert!(json.contains("\"status\": \"diverted\""));
        assert!(json.contains("\"requested_branch\": \"feature-login\""));
        assert!(json.contains("\"occupied_by\": \"qa-agent-01\""));
        assert!(json.contains("\"actual_branch\": \"feature-login--coder-01\""));
    }
}
