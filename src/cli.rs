use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "garc", version, about = "Git Agent Race Controller")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    #[must_use]
    pub fn json_output(&self) -> bool {
        match &self.command {
            Command::Init(args) => args.json,
            Command::Checkout(args) => args.json,
            Command::Status(args) => args.json,
            Command::Up(args) => args.json,
            Command::Commit(args) => args.json,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Install the post-checkout hook and validate local CAMP configuration.
    Init(InitArgs),
    /// Smart checkout that avoids branch collisions on the mesh.
    Checkout(CheckoutArgs),
    /// Show local git state alongside discovered mesh peers.
    Status(StatusArgs),
    /// Bring this agent online by delegating to `camp up`.
    Up(UpArgs),
    /// Mesh-guarded `git commit`: verifies branch is uncontested before committing.
    Commit(CommitArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Path to the CAMP config file.
    #[arg(long, default_value = ".camp.toml")]
    pub config: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CheckoutArgs {
    /// Branch to check out.
    pub branch: String,
    /// Path to the CAMP config file.
    #[arg(long, default_value = ".camp.toml")]
    pub config: PathBuf,
    /// Override the claim settle window in milliseconds for this invocation.
    #[arg(long)]
    pub claim_settle_ms: Option<u64>,
    /// Bypass mesh collision checks.
    #[arg(long)]
    pub force: bool,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Path to the CAMP config file.
    #[arg(long, default_value = ".camp.toml")]
    pub config: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct UpArgs {
    /// Path to the CAMP config file.
    #[arg(long, default_value = ".camp.toml")]
    pub config: PathBuf,
    /// Emit machine-readable JSON preamble before delegating to camp.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CommitArgs {
    /// Path to the CAMP config file.
    #[arg(long, default_value = ".camp.toml")]
    pub config: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
    /// Arguments forwarded verbatim to `git commit` after the mesh guard passes.
    #[arg(last = true)]
    pub passthrough: Vec<String>,
}
