mod cli;
mod config;
mod engine;
mod errors;
mod git;
mod installer;
mod mesh;
mod output;

use std::{
    collections::BTreeMap,
    env,
    process::{Command as ProcessCommand, ExitCode},
    time::Instant,
};

use anyhow::{Context, Result};
use clap::Parser;

use crate::{
    cli::{Cli, Command},
    config::{CampConfig, resolve_config_path},
    engine::{
        CollisionResult, active_branch_occupier, claim_winner, detect_collision,
        diverted_branch_name, observed_claimants,
    },
    errors::GarcError,
    git::{
        checkout_diverted_branch, checkout_existing_branch, checkout_force_branch, current_branch,
        open_repo_from,
    },
    installer::install_post_checkout_hook,
    mesh::{
        LocalClaimState, discover_peers, discover_peers_with_retry,
        discover_peers_with_retry_metadata, publish_branch_claim, read_local_claim_state,
        update_local_branch,
    },
    output::{
        ActiveClaimSummary, CheckoutOutput, CheckoutStatus, CommitOutput, CommitStatus,
        DecisionBasis, DecisionTraceEntry, InitOutput, ObservedPeerOutput, OccupiedBranchSummary,
        StatusOutput, UpOutput, print_checkout, print_commit, print_error, print_init,
        print_status, print_up,
    },
};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if cli.json_output() {
                if let Err(print_error) = print_error(error.to_string()) {
                    eprintln!("error: {error:#}");
                    eprintln!("error: failed to render JSON error output: {print_error:#}");
                }
            } else {
                eprintln!("error: {error:#}");
            }

            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    match &cli.command {
        Command::Init(args) => run_init(args.json, &args.config),
        Command::Checkout(args) => run_checkout(
            args.json,
            &args.config,
            &args.branch,
            args.force,
            args.claim_settle_ms,
        ),
        Command::Status(args) => run_status(args.json, &args.config),
        Command::Up(args) => run_up(args.json, &args.config),
        Command::Commit(args) => run_commit(args.json, &args.config, &args.passthrough),
    }
}

fn run_init(json: bool, config_arg: &std::path::Path) -> Result<()> {
    let repo = open_repo_from(&env::current_dir().context("failed to resolve current directory")?)?;
    let config_path = resolve_config_path(&repo.repo_root, config_arg);
    let mut config = CampConfig::from_path(&config_path)?;
    let project = validated_project(&repo.project_name, &config, &config_path)?;
    let branch = current_branch(&repo.repo)?;

    update_local_branch(&config_path, &mut config, &branch)?;
    let hook_path = install_post_checkout_hook(&repo.git_dir)?;

    let output = InitOutput {
        status: "initialized",
        agent_id: config.agent.id,
        project,
        hook_path: hook_path.display().to_string(),
        message: "Installed garc post-checkout hook and synchronized CAMP branch metadata."
            .to_owned(),
    };
    print_init(&output, json)
}

fn run_checkout(
    json: bool,
    config_arg: &std::path::Path,
    requested_branch: &str,
    force: bool,
    claim_settle_ms_override: Option<u64>,
) -> Result<()> {
    let repo = open_repo_from(&env::current_dir().context("failed to resolve current directory")?)?;
    let config_path = resolve_config_path(&repo.repo_root, config_arg);
    let mut config = CampConfig::from_path(&config_path)?;
    let project = validated_project(&repo.project_name, &config, &config_path)?;
    let claim_settle_ms =
        resolve_claim_settle_ms(config.claim_settle_ms(), claim_settle_ms_override);

    let output = if force {
        let started_at = Instant::now();
        let mut decision_trace_entries = Vec::new();
        record_trace_entry(
            &mut decision_trace_entries,
            &started_at,
            "force_bypass".to_owned(),
        );
        checkout_force_branch(&repo.repo, requested_branch)?;
        update_local_state(&config_path, &mut config, requested_branch)?;
        record_trace_entry(
            &mut decision_trace_entries,
            &started_at,
            "decision:checked_out".to_owned(),
        );

        CheckoutOutput {
            status: CheckoutStatus::Forced,
            requested_branch: requested_branch.to_owned(),
            occupied_by: None,
            decision_basis: DecisionBasis::ForceBypass,
            observed_claims: Vec::new(),
            observed_peers: Vec::new(),
            claim_winner: None,
            decision_trace: vec!["force_bypass".to_owned(), "decision:checked_out".to_owned()],
            decision_trace_entries,
            actual_branch: requested_branch.to_owned(),
            message: "Bypassed mesh collision checks and checked out the requested branch."
                .to_owned(),
        }
    } else {
        let started_at = Instant::now();
        let claim =
            publish_branch_claim(&config, &repo.git_dir, requested_branch, claim_settle_ms)?;
        let mut decision_trace = vec![
            format!("published_claim:{requested_branch}"),
            format!("claim_settle_ms:{claim_settle_ms}"),
        ];
        let mut decision_trace_entries = Vec::new();
        record_trace_entry(
            &mut decision_trace_entries,
            &started_at,
            format!("published_claim:{requested_branch}"),
        );
        record_trace_entry(
            &mut decision_trace_entries,
            &started_at,
            format!("claim_settle_ms:{claim_settle_ms}"),
        );
        if claim.settle_required() {
            claim.settle();
            decision_trace.push("claim_settle_complete".to_owned());
            record_trace_entry(
                &mut decision_trace_entries,
                &started_at,
                "claim_settle_complete".to_owned(),
            );
        }
        let (peers, read_attempts) = discover_peers_with_retry_metadata(&config)?;
        decision_trace.push(format!("observed_peer_count:{}", peers.len()));
        decision_trace.push(format!("mesh_read_attempts:{read_attempts}"));
        record_trace_entry(
            &mut decision_trace_entries,
            &started_at,
            format!("observed_peer_count:{}", peers.len()),
        );
        record_trace_entry(
            &mut decision_trace_entries,
            &started_at,
            format!("mesh_read_attempts:{read_attempts}"),
        );
        let observed_claims =
            observed_claimants(&peers, &project, requested_branch, &config.agent.id);
        let observed_peers = observed_peer_outputs(&peers, &project);
        let active_occupier =
            active_branch_occupier(&peers, &project, requested_branch, &config.agent.id);
        let claim_winner = if observed_claims.is_empty() {
            None
        } else {
            claim_winner(&peers, &project, requested_branch, &config.agent.id)
        };
        match detect_collision(&peers, &project, requested_branch, &config.agent.id) {
            CollisionResult::Clear => {
                checkout_existing_branch(&repo.repo, requested_branch)?;
                update_local_state(&config_path, &mut config, requested_branch)?;
                if observed_claims.is_empty() {
                    decision_trace.push("mesh_clear".to_owned());
                    record_trace_entry(
                        &mut decision_trace_entries,
                        &started_at,
                        "mesh_clear".to_owned(),
                    );
                } else if let Some(claim_winner) = &claim_winner {
                    decision_trace.push(format!("claim_winner:{claim_winner}"));
                    record_trace_entry(
                        &mut decision_trace_entries,
                        &started_at,
                        format!("claim_winner:{claim_winner}"),
                    );
                }
                decision_trace.push("decision:checked_out".to_owned());
                record_trace_entry(
                    &mut decision_trace_entries,
                    &started_at,
                    "decision:checked_out".to_owned(),
                );

                CheckoutOutput {
                    status: CheckoutStatus::CheckedOut,
                    requested_branch: requested_branch.to_owned(),
                    occupied_by: None,
                    decision_basis: if observed_claims.is_empty() {
                        DecisionBasis::MeshClear
                    } else {
                        DecisionBasis::ClaimArbitrationWon
                    },
                    observed_claims,
                    observed_peers,
                    claim_winner,
                    decision_trace,
                    decision_trace_entries,
                    actual_branch: requested_branch.to_owned(),
                    message: "Target branch is clear on the mesh. Checked out requested branch."
                        .to_owned(),
                }
            }
            CollisionResult::Occupied { by } => {
                let actual_branch = diverted_branch_name(requested_branch, &config.agent.id);
                checkout_diverted_branch(&repo.repo, requested_branch, &actual_branch)?;
                update_local_state(&config_path, &mut config, &actual_branch)?;
                if active_occupier.is_some() {
                    decision_trace.push(format!("active_occupier:{by}"));
                    record_trace_entry(
                        &mut decision_trace_entries,
                        &started_at,
                        format!("active_occupier:{by}"),
                    );
                } else {
                    decision_trace.push(format!("claim_winner:{by}"));
                    record_trace_entry(
                        &mut decision_trace_entries,
                        &started_at,
                        format!("claim_winner:{by}"),
                    );
                }
                decision_trace.push(format!("decision:diverted:{actual_branch}"));
                record_trace_entry(
                    &mut decision_trace_entries,
                    &started_at,
                    format!("decision:diverted:{actual_branch}"),
                );

                CheckoutOutput {
                    status: CheckoutStatus::Diverted,
                    requested_branch: requested_branch.to_owned(),
                    occupied_by: Some(by),
                    decision_basis: if active_occupier.is_some() {
                        DecisionBasis::BranchOccupied
                    } else {
                        DecisionBasis::ClaimArbitrationLost
                    },
                    observed_claims,
                    observed_peers,
                    claim_winner,
                    decision_trace,
                    decision_trace_entries,
                    actual_branch,
                    message: "Target branch is currently locked. Checked out sub-branch to prevent race conditions.".to_owned(),
                }
            }
        }
    };

    print_checkout(&output, json)
}

fn run_status(json: bool, config_arg: &std::path::Path) -> Result<()> {
    let repo = open_repo_from(&env::current_dir().context("failed to resolve current directory")?)?;
    let config_path = resolve_config_path(&repo.repo_root, config_arg);
    let mut config = CampConfig::from_path(&config_path)?;
    let project = validated_project(&repo.project_name, &config, &config_path)?;
    let branch = current_branch(&repo.repo)?;
    update_local_branch(&config_path, &mut config, &branch)?;
    let peers = discover_peers(&config)?
        .into_iter()
        .filter(|peer| peer.current_project == project)
        .collect::<Vec<_>>();
    let local_claim_state = read_local_claim_state(&repo.git_dir)?;

    let output = StatusOutput {
        status: "ok",
        agent_id: config.agent.id.clone(),
        project,
        local_branch: branch.clone(),
        occupied_branches: occupied_branch_summaries(&config.agent.id, &branch, &peers),
        active_claims: active_claim_summaries(&config.agent.id, local_claim_state.as_ref(), &peers),
        peers,
    };
    print_status(&output, json)
}

fn run_up(json: bool, config_arg: &std::path::Path) -> Result<()> {
    let repo = open_repo_from(&env::current_dir().context("failed to resolve current directory")?)?;
    let config_path = resolve_config_path(&repo.repo_root, config_arg);
    let mut config = CampConfig::from_path(&config_path)?;
    let project = validated_project(&repo.project_name, &config, &config_path)?;
    let branch = current_branch(&repo.repo)?;
    update_local_branch(&config_path, &mut config, &branch)?;

    let delegated_command = format!("camp up --config {}", config_path.display());
    let output = UpOutput {
        status: "delegating",
        agent_id: config.agent.id.clone(),
        project,
        branch,
        delegated_command: delegated_command.clone(),
        message: "Validated repo state and delegating mesh presence to `camp up`.".to_owned(),
    };

    if json {
        print_up(&output, true)?;
    }

    let mut command = ProcessCommand::new("camp");
    command.arg("up").arg("--config").arg(&config_path);
    if json {
        command.arg("--json");
    }

    let status = command.status().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!(
                "failed to execute `camp up`; `camp` was not found in $PATH. Install it with `bash <(curl -fsSL https://raw.githubusercontent.com/0xBoji/coding_agent_mesh_presence/main/scripts/install.sh)`"
            )
        } else {
            anyhow::anyhow!("failed to execute `{delegated_command}`: {error}")
        }
    })?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "`{delegated_command}` exited with status {}",
            status
                .code()
                .map_or_else(|| "signal".to_owned(), |code| code.to_string())
        ))
    }
}

fn run_commit(json: bool, config_arg: &std::path::Path, passthrough: &[String]) -> Result<()> {
    let repo = open_repo_from(&env::current_dir().context("failed to resolve current directory")?)?;
    let config_path = resolve_config_path(&repo.repo_root, config_arg);
    let mut config = CampConfig::from_path(&config_path)?;
    let project = validated_project(&repo.project_name, &config, &config_path)?;
    let branch = current_branch(&repo.repo)?;
    update_local_branch(&config_path, &mut config, &branch)?;

    let peers = discover_peers_with_retry(&config)?;
    match detect_collision(&peers, &project, &branch, &config.agent.id) {
        CollisionResult::Clear => {}
        CollisionResult::Occupied { by } => {
            return Err(anyhow::anyhow!(
                "refusing to commit on contested branch `{branch}`; occupied by `{by}`"
            ));
        }
    }

    let mut command = ProcessCommand::new("git");
    command
        .current_dir(&repo.repo_root)
        .arg("commit")
        .args(passthrough);

    if json {
        let output = command.output().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("failed to execute `git commit`; `git` was not found in $PATH")
            } else {
                anyhow::anyhow!("failed to execute `git commit`: {error}")
            }
        })?;

        if output.status.success() {
            let commit_output = CommitOutput {
                status: CommitStatus::Committed,
                branch,
                git_exit_code: output.status.code().unwrap_or(0),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                message: "Mesh clear. Executed git commit.".to_owned(),
            };
            print_commit(&commit_output, true)
        } else {
            Err(anyhow::anyhow!(
                "git commit exited with status {}: {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    } else {
        let status = command.status().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("failed to execute `git commit`; `git` was not found in $PATH")
            } else {
                anyhow::anyhow!("failed to execute `git commit`: {error}")
            }
        })?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "`git commit` exited with status {}",
                status
                    .code()
                    .map_or_else(|| "signal".to_owned(), |code| code.to_string())
            ))
        }
    }
}

fn resolve_claim_settle_ms(config_value: u64, override_value: Option<u64>) -> u64 {
    override_value.unwrap_or(config_value)
}

fn record_trace_entry(entries: &mut Vec<DecisionTraceEntry>, started_at: &Instant, event: String) {
    entries.push(DecisionTraceEntry {
        event,
        at_ms: started_at.elapsed().as_millis() as u64,
    });
}

fn occupied_branch_summaries(
    local_agent_id: &str,
    local_branch: &str,
    peers: &[crate::mesh::MeshPeer],
) -> Vec<OccupiedBranchSummary> {
    let mut branches = BTreeMap::<String, Vec<String>>::new();
    branches
        .entry(local_branch.to_owned())
        .or_default()
        .push(local_agent_id.to_owned());
    for peer in peers {
        branches
            .entry(peer.current_branch.clone())
            .or_default()
            .push(peer.agent_id.clone());
    }

    branches
        .into_iter()
        .map(|(branch, mut occupied_by)| {
            occupied_by.sort();
            occupied_by.dedup();
            OccupiedBranchSummary {
                branch,
                occupied_by,
            }
        })
        .collect()
}

fn observed_peer_outputs(
    peers: &[crate::mesh::MeshPeer],
    project: &str,
) -> Vec<ObservedPeerOutput> {
    peers
        .iter()
        .filter(|peer| peer.current_project == project)
        .map(|peer| ObservedPeerOutput {
            agent_id: peer.agent_id.clone(),
            current_branch: peer.current_branch.clone(),
            intent_branch: peer.intent_branch.clone(),
        })
        .collect()
}

fn active_claim_summaries(
    local_agent_id: &str,
    local_claim_state: Option<&LocalClaimState>,
    peers: &[crate::mesh::MeshPeer],
) -> Vec<ActiveClaimSummary> {
    let mut claims = BTreeMap::<String, Vec<String>>::new();
    if let Some(local_claim_state) = local_claim_state {
        claims
            .entry(local_claim_state.intent_branch.clone())
            .or_default()
            .push(local_agent_id.to_owned());
    }
    for peer in peers {
        if let Some(intent_branch) = &peer.intent_branch {
            claims
                .entry(intent_branch.clone())
                .or_default()
                .push(peer.agent_id.clone());
        }
    }

    claims
        .into_iter()
        .map(|(branch, mut claimants)| {
            claimants.sort();
            claimants.dedup();
            let claim_winner = claimants
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown".to_owned());
            ActiveClaimSummary {
                branch,
                claimants,
                claim_winner,
            }
        })
        .collect()
}

fn validated_project(
    repo_project: &str,
    config: &CampConfig,
    config_path: &std::path::Path,
) -> Result<String> {
    let config_project = config.agent.project.as_str();
    if config_project == repo_project {
        Ok(config_project.to_owned())
    } else {
        Err(GarcError::ProjectMismatch {
            config_path: config_path.display().to_string(),
            config_project: config_project.to_owned(),
            repo_project: repo_project.to_owned(),
        }
        .into())
    }
}

fn update_local_state(
    config_path: &std::path::Path,
    config: &mut CampConfig,
    branch: &str,
) -> Result<()> {
    update_local_branch(config_path, config, branch)?;
    let _ = ProcessCommand::new("camp")
        .args(["update", "--branch", branch])
        .status();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_claim_settle_ms;

    #[test]
    fn cli_override_wins_over_config_claim_settle_ms() {
        assert_eq!(resolve_claim_settle_ms(150, Some(25)), 25);
        assert_eq!(resolve_claim_settle_ms(150, None), 150);
    }
}
