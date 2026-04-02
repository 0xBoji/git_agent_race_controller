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
