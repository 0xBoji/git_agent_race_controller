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
fn trace_returns_structured_empty_json_when_no_trace_exists() -> Result<()> {
    let harness = TestRepo::new("_garc-trace-empty._tcp.local.")?;

    let output = harness.run(["trace", "--json"])?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "empty");
    assert!(json["latest"].is_null());
    assert_eq!(json["history"].as_array().map(Vec::len), Some(0));
    Ok(())
}

#[test]
fn trace_returns_latest_and_history_json() -> Result<()> {
    let harness = TestRepo::new("_garc-trace-history._tcp.local.")?;
    harness.write_trace("feature-a", "checked_out")?;
    harness.write_trace("feature-b", "diverted")?;

    let output = harness.run(["trace", "--history", "--json"])?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "ok");
    assert_eq!(json["latest"]["requested_branch"], "feature-b");
    assert_eq!(json["history"].as_array().map(Vec::len), Some(2));
    assert_eq!(json["history"][0]["requested_branch"], "feature-b");
    assert_eq!(json["history"][1]["requested_branch"], "feature-a");
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
fn checkout_diverts_when_remote_agent_wins_branch_claim_arbitration() -> Result<()> {
    let harness = TestRepo::new("_garc-checkout-claim-divert._tcp.local.")?;
    harness.create_branch("feature-login")?;
    let output = harness.run_with_snapshot(
        ["checkout", "feature-login", "--json"],
        json!([{
            "agent_id": "alpha-agent",
            "current_branch": "main",
            "current_project": harness.project_name,
            "intent_branch": "feature-login",
            "fullname": "garc-claim-alpha-agent._camp._tcp.local.",
            "port": 7000
        }]),
    )?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "diverted");
    assert_eq!(json["decision_basis"], "claim_arbitration_lost");
    assert_eq!(json["claim_winner"], "alpha-agent");
    assert_eq!(json["occupied_by"], "alpha-agent");
    assert_eq!(json["observed_claims"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["observed_claims"][0], "alpha-agent");
    assert_eq!(json["observed_peers"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["observed_peers"][0]["agent_id"], "alpha-agent");
    assert_eq!(json["observed_peers"][0]["current_branch"], "main");
    assert_eq!(json["observed_peers"][0]["intent_branch"], "feature-login");
    assert_eq!(json["mesh_read_attempts"], 1);
    assert_eq!(
        json["mesh_read_backoff_ms"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(json["decision_trace"].as_array().map(Vec::len), Some(6));
    assert_eq!(json["decision_trace"][0], "published_claim:feature-login");
    assert_eq!(
        json["decision_trace_entries"].as_array().map(Vec::len),
        Some(6)
    );
    assert_eq!(
        json["decision_trace_entries"][0]["event"],
        "published_claim:feature-login"
    );
    assert!(
        json["decision_trace_entries"][0]["at_ms"]
            .as_u64()
            .is_some()
    );
    assert_eq!(json["actual_branch"], "feature-login--local-agent");
    assert_eq!(harness.current_branch()?, "feature-login--local-agent");
    Ok(())
}

#[test]
fn checkout_stays_on_requested_branch_when_local_agent_wins_branch_claim_arbitration() -> Result<()>
{
    let harness = TestRepo::new("_garc-checkout-claim-clear._tcp.local.")?;
    harness.create_branch("feature-login")?;
    let output = harness.run_with_snapshot(
        ["checkout", "feature-login", "--json"],
        json!([{
            "agent_id": "zeta-agent",
            "current_branch": "main",
            "current_project": harness.project_name,
            "intent_branch": "feature-login",
            "fullname": "garc-claim-zeta-agent._camp._tcp.local.",
            "port": 7000
        }]),
    )?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "checked_out");
    assert_eq!(json["decision_basis"], "claim_arbitration_won");
    assert_eq!(json["claim_winner"], "local-agent");
    assert_eq!(json["observed_claims"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["observed_claims"][0], "zeta-agent");
    assert_eq!(json["actual_branch"], "feature-login");
    assert_eq!(harness.current_branch()?, "feature-login");
    Ok(())
}

#[test]
fn force_checkout_bypasses_remote_branch_claim_arbitration() -> Result<()> {
    let harness = TestRepo::new("_garc-checkout-claim-force._tcp.local.")?;
    harness.create_branch("feature-login")?;
    let output = harness.run_with_snapshot(
        ["checkout", "feature-login", "--force", "--json"],
        json!([{
            "agent_id": "alpha-agent",
            "current_branch": "main",
            "current_project": harness.project_name,
            "intent_branch": "feature-login",
            "fullname": "garc-claim-alpha-agent._camp._tcp.local.",
            "port": 7000
        }]),
    )?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "forced");
    assert_eq!(json["decision_basis"], "force_bypass");
    assert_eq!(json["actual_branch"], "feature-login");
    assert_eq!(harness.current_branch()?, "feature-login");
    Ok(())
}

#[test]
fn checkout_accepts_claim_settle_ms_cli_override() -> Result<()> {
    let harness = TestRepo::new("_garc-checkout-claim-override._tcp.local.")?;
    harness.create_branch("feature-login")?;

    let output = harness.run_with_snapshot(
        [
            "checkout",
            "feature-login",
            "--claim-settle-ms",
            "5",
            "--json",
        ],
        json!([]),
    )?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "checked_out");
    assert_eq!(json["actual_branch"], "feature-login");
    let trace_path = harness.repo_dir.join(".git/garc/last-checkout-trace.json");
    assert!(trace_path.exists());
    let history_dir = harness.repo_dir.join(".git/garc/trace-history");
    assert!(history_dir.exists());
    assert_eq!(fs::read_dir(&history_dir)?.count(), 1);
    let persisted_trace: Value = serde_json::from_slice(&fs::read(trace_path)?)?;
    assert_eq!(persisted_trace["requested_branch"], "feature-login");
    assert_eq!(persisted_trace["status"], "checked_out");
    Ok(())
}

#[test]
fn checkout_fails_closed_when_claim_publication_cannot_start() -> Result<()> {
    let harness = TestRepo::new("_garc-invalid-service._tcp.local.")?;
    harness.create_branch("feature-login")?;
    harness.write_config_with_discovery(&harness.project_name, "invalid-service-type", None)?;

    let output = harness.run(["checkout", "feature-login", "--json"])?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(!output.status.success());
    assert_eq!(json["status"], "error");
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("claim") || message.contains("service"))
    );
    assert_eq!(harness.current_branch()?, "main");
    Ok(())
}

#[test]
fn status_reports_local_branch_and_mesh_peers() -> Result<()> {
    let harness = TestRepo::new("_garc-status-test._tcp.local.")?;
    harness.write_local_claim_state("feature-login")?;
    let output = harness.run_with_snapshot(
        ["status", "--json"],
        json!([
            {
                "agent_id": "reviewer-01",
                "current_branch": "main",
                "current_project": harness.project_name,
                "intent_branch": "feature-login",
                "fullname": "reviewer-01._camp._tcp.local.",
                "port": 7000
            },
            {
                "agent_id": "other-project-agent",
                "current_branch": "main",
                "current_project": "some-other-project",
                "fullname": "other-project-agent._camp._tcp.local.",
                "port": 7001
            }
        ]),
    )?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(output.status.success());
    assert_eq!(json["status"], "ok");
    assert_eq!(json["local_branch"], "main");
    assert_eq!(json["peers"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["peers"][0]["agent_id"], "reviewer-01");
    assert_eq!(json["peers"][0]["intent_branch"], "feature-login");
    assert_eq!(json["occupied_branches"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["occupied_branches"][0]["branch"], "main");
    assert_eq!(
        json["occupied_branches"][0]["occupied_by"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        json["occupied_branches"][0]["occupied_by"][0],
        "local-agent"
    );
    assert_eq!(
        json["occupied_branches"][0]["occupied_by"][1],
        "reviewer-01"
    );
    assert_eq!(json["active_claims"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["active_claims"][0]["branch"], "feature-login");
    assert_eq!(
        json["active_claims"][0]["claimants"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(json["active_claims"][0]["claimants"][0], "local-agent");
    assert_eq!(json["active_claims"][0]["claimants"][1], "reviewer-01");
    assert_eq!(json["active_claims"][0]["claim_winner"], "local-agent");
    Ok(())
}

#[test]
fn project_mismatch_returns_structured_json_error() -> Result<()> {
    let harness = TestRepo::new("_garc-project-mismatch._tcp.local.")?;
    harness.write_config_project("different-project")?;

    let output = harness.run(["status", "--json"])?;
    let json: Value = serde_json::from_slice(&output.stdout)?;

    assert!(!output.status.success());
    assert_eq!(json["status"], "error");
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("does not match repository project"))
    );
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

    fn write_config_project(&self, project: &str) -> Result<()> {
        self.write_config_with_discovery(project, "_garc-project-mismatch._tcp.local.", None)
    }

    fn write_config_with_discovery(
        &self,
        project: &str,
        service_type: &str,
        mdns_port: Option<u16>,
    ) -> Result<()> {
        let mdns_port_line = mdns_port
            .map(|port| format!("mdns_port = {port}\n"))
            .unwrap_or_default();
        let config = format!(
            "[agent]\nid = \"local-agent\"\nproject = \"{project}\"\nbranch = \"main\"\n\n[discovery]\nservice_type = \"{service_type}\"\n{mdns_port_line}discovery_timeout_ms = 3000\n"
        );
        fs::write(self.repo_dir.join(".camp.toml"), config)?;
        Ok(())
    }

    fn write_local_claim_state(&self, branch: &str) -> Result<()> {
        let garc_dir = self.repo_dir.join(".git").join("garc");
        fs::create_dir_all(&garc_dir)?;
        fs::write(
            garc_dir.join("claim-state.json"),
            format!(
                "{{\n  \"agent_id\": \"local-agent\",\n  \"current_project\": \"{}\",\n  \"current_branch\": \"main\",\n  \"intent_branch\": \"{}\"\n}}\n",
                self.project_name, branch
            ),
        )?;
        Ok(())
    }

    fn write_trace(&self, branch: &str, status: &str) -> Result<()> {
        let garc_dir = self.repo_dir.join(".git").join("garc");
        let history_dir = garc_dir.join("trace-history");
        fs::create_dir_all(&history_dir)?;

        let trace = format!(
            "{{\n  \"status\": \"{status}\",\n  \"requested_branch\": \"{branch}\",\n  \"actual_branch\": \"{branch}\",\n  \"message\": \"trace\"\n}}\n"
        );
        fs::write(garc_dir.join("last-checkout-trace.json"), &trace)?;

        let next_index = fs::read_dir(&history_dir)?.count() + 1;
        fs::write(history_dir.join(format!("{next_index:020}.json")), trace)?;
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
