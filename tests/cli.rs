use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Result;
use git2::{Repository, Signature, build::CheckoutBuilder};
use serde_json::{Value, json};
use tempfile::TempDir;

#[test]
fn init_installs_hook_and_returns_json() -> Result<()> {
    let harness = TestRepo::new("_garc-init-test._tcp.local.")?;

    let output = harness.run(["init", "--json"])?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "initialized");
    assert!(harness.hook_path().exists());
    let hook = fs::read_to_string(harness.hook_path())?;
    assert!(hook.contains("garc managed start"));
    Ok(())
}

#[test]
fn checkout_returns_json_when_branch_is_clear() -> Result<()> {
    let harness = TestRepo::new("_garc-checkout-clear._tcp.local.")?;
    harness.create_branch("feature-login")?;

    let output = harness.run(["checkout", "feature-login", "--json"])?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "checked_out");
    assert_eq!(json["actual_branch"], "feature-login");
    assert_eq!(harness.current_branch()?, "feature-login");
    Ok(())
}

#[test]
fn checkout_diverts_when_remote_agent_occupies_branch() -> Result<()> {
    let harness = TestRepo::new("_garc-checkout-divert._tcp.local.")?;
    harness.create_branch("feature-login")?;
    let output = harness.run_with_snapshot(
        ["checkout", "feature-login", "--json"],
        json!([{
            "agent_id": "qa-agent-01",
            "current_branch": "feature-login",
            "current_project": harness.project_name,
            "fullname": "qa-agent-01._camp._tcp.local.",
            "port": 7000
        }]),
    )?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "diverted");
    assert_eq!(json["occupied_by"], "qa-agent-01");
    assert_eq!(json["actual_branch"], "feature-login--local-agent");
    assert_eq!(harness.current_branch()?, "feature-login--local-agent");
    Ok(())
}

#[test]
fn status_reports_local_branch_and_mesh_peers() -> Result<()> {
    let harness = TestRepo::new("_garc-status-test._tcp.local.")?;
    let output = harness.run_with_snapshot(
        ["status", "--json"],
        json!([{
            "agent_id": "reviewer-01",
            "current_branch": "main",
            "current_project": harness.project_name,
            "fullname": "reviewer-01._camp._tcp.local.",
            "port": 7000
        }]),
    )?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "ok");
    assert_eq!(json["local_branch"], "main");
    assert_eq!(json["peers"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["peers"][0]["agent_id"], "reviewer-01");
    Ok(())
}

#[test]
fn missing_config_returns_structured_json_error() -> Result<()> {
    let harness = TestRepo::new("_garc-missing-config._tcp.local.")?;
    fs::remove_file(harness.repo_dir.join(".camp.toml"))?;

    let output = harness.run(["status", "--json"])?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(!output.status.success());
    assert_eq!(json["status"], "error");
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains(".camp.toml"))
    );
    Ok(())
}

struct TestRepo {
    _tempdir: TempDir,
    repo_dir: PathBuf,
    project_name: String,
}

impl TestRepo {
    fn new(service_type: &str) -> Result<Self> {
        let tempdir = TempDir::new()?;
        let repo_dir = tempdir.path().join("repo-under-test");
        fs::create_dir_all(&repo_dir)?;
        let repo = Repository::init(&repo_dir)?;
        fs::write(repo_dir.join("README.md"), "hello\n")?;

        let mut index = repo.index()?;
        index.add_path(Path::new("README.md"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let signature = Signature::now("garc", "garc@example.com")?;
        repo.commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])?;
        drop(tree);

        let commit = repo.head()?.peel_to_commit()?;
        repo.branch("main", &commit, false)?;
        repo.set_head("refs/heads/main")?;
        let mut checkout = CheckoutBuilder::new();
        checkout.force();
        repo.checkout_head(Some(&mut checkout))?;

        let project_name = repo_dir
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("repo-under-test")
            .to_owned();
        let config = format!(
            "[agent]\nid = \"local-agent\"\nproject = \"{project_name}\"\nbranch = \"main\"\n\n[discovery]\nservice_type = \"{service_type}\"\ndiscovery_timeout_ms = 900\n"
        );
        fs::write(repo_dir.join(".camp.toml"), config)?;

        Ok(Self {
            _tempdir: tempdir,
            repo_dir,
            project_name,
        })
    }

    fn run<I, S>(&self, args: I) -> Result<std::process::Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        Ok(Command::new(env!("CARGO_BIN_EXE_garc"))
            .args(args)
            .current_dir(&self.repo_dir)
            .output()?)
    }

    fn run_with_snapshot<I, S>(&self, args: I, snapshot: Value) -> Result<std::process::Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        Ok(Command::new(env!("CARGO_BIN_EXE_garc"))
            .args(args)
            .env("GARC_MESH_SNAPSHOT_JSON", serde_json::to_string(&snapshot)?)
            .current_dir(&self.repo_dir)
            .output()?)
    }

    fn create_branch(&self, branch: &str) -> Result<()> {
        let repo = Repository::open(&self.repo_dir)?;
        let commit = repo.head()?.peel_to_commit()?;
        repo.branch(branch, &commit, false)?;
        Ok(())
    }

    fn current_branch(&self) -> Result<String> {
        let repo = Repository::open(&self.repo_dir)?;
        Ok(repo.head()?.shorthand().unwrap_or_default().to_owned())
    }

    fn hook_path(&self) -> PathBuf {
        self.repo_dir.join(".git/hooks/post-checkout")
    }
}
