mod cli;
mod config;
mod engine;
mod errors;
mod git;
mod installer;
mod mesh;
mod output;

use std::{
    env,
    process::{Command as ProcessCommand, ExitCode},
};

use anyhow::{Context, Result};
use clap::Parser;

use crate::{
    cli::{Cli, Command},
    config::{CampConfig, resolve_config_path},
    engine::{CollisionResult, detect_collision, diverted_branch_name},
    git::{
        checkout_diverted_branch, checkout_existing_branch, checkout_force_branch, current_branch,
        open_repo_from,
    },
    installer::install_post_checkout_hook,
    mesh::{discover_peers, update_local_branch},
    output::{
        CheckoutOutput, CheckoutStatus, InitOutput, StatusOutput, print_checkout, print_error,
        print_init, print_status,
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
        Command::Checkout(args) => run_checkout(args.json, &args.config, &args.branch, args.force),
        Command::Status(args) => run_status(args.json, &args.config),
    }
}

fn run_init(json: bool, config_arg: &std::path::Path) -> Result<()> {
    let repo = open_repo_from(&env::current_dir().context("failed to resolve current directory")?)?;
    let config_path = resolve_config_path(&repo.repo_root, config_arg);
    let mut config = CampConfig::from_path(&config_path)?;
    let branch = current_branch(&repo.repo)?;

    update_local_branch(&config_path, &mut config, &branch)?;
    let hook_path = install_post_checkout_hook(&repo.git_dir)?;

    let output = InitOutput {
        status: "initialized",
        agent_id: config.agent.id,
        project: repo.project_name,
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
) -> Result<()> {
    let repo = open_repo_from(&env::current_dir().context("failed to resolve current directory")?)?;
    let config_path = resolve_config_path(&repo.repo_root, config_arg);
    let mut config = CampConfig::from_path(&config_path)?;

    let output = if force {
        checkout_force_branch(&repo.repo, requested_branch)?;
        update_local_state(&config_path, &mut config, requested_branch)?;

        CheckoutOutput {
            status: CheckoutStatus::Forced,
            requested_branch: requested_branch.to_owned(),
            occupied_by: None,
            actual_branch: requested_branch.to_owned(),
            message: "Bypassed mesh collision checks and checked out the requested branch."
                .to_owned(),
        }
    } else {
        let peers = discover_peers(&config)?;
        match detect_collision(
            &peers,
            &repo.project_name,
            requested_branch,
            &config.agent.id,
        ) {
            CollisionResult::Clear => {
                checkout_existing_branch(&repo.repo, requested_branch)?;
                update_local_state(&config_path, &mut config, requested_branch)?;

                CheckoutOutput {
                    status: CheckoutStatus::CheckedOut,
                    requested_branch: requested_branch.to_owned(),
                    occupied_by: None,
                    actual_branch: requested_branch.to_owned(),
                    message: "Target branch is clear on the mesh. Checked out requested branch."
                        .to_owned(),
                }
            }
            CollisionResult::Occupied { by } => {
                let actual_branch = diverted_branch_name(requested_branch, &config.agent.id);
                checkout_diverted_branch(&repo.repo, requested_branch, &actual_branch)?;
                update_local_state(&config_path, &mut config, &actual_branch)?;

                CheckoutOutput {
                    status: CheckoutStatus::Diverted,
                    requested_branch: requested_branch.to_owned(),
                    occupied_by: Some(by),
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
    let branch = current_branch(&repo.repo)?;
    update_local_branch(&config_path, &mut config, &branch)?;
    let peers = discover_peers(&config)?;

    let output = StatusOutput {
        status: "ok",
        agent_id: config.agent.id,
        project: repo.project_name,
        local_branch: branch,
        peers,
    };
    print_status(&output, json)
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
