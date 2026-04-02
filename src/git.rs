use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use git2::{BranchType, Commit, ObjectType, Repository, build::CheckoutBuilder};

pub struct RepoContext {
    pub repo: Repository,
    pub repo_root: PathBuf,
    pub git_dir: PathBuf,
    pub project_name: String,
}

pub fn open_repo_from(start: &Path) -> Result<RepoContext> {
    let repo = Repository::discover(start).context("failed to discover git repository")?;
    let repo_root = repo
        .workdir()
        .map(Path::to_path_buf)
        .or_else(|| repo.path().parent().map(Path::to_path_buf))
        .context("repository has no workdir")?;
    let git_dir = repo.path().to_path_buf();
    let project_name = repo_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .context("failed to derive project name from repository path")?;

    Ok(RepoContext {
        repo,
        repo_root,
        git_dir,
        project_name,
    })
}

pub fn current_branch(repo: &Repository) -> Result<String> {
    let head = repo.head().context("failed to read HEAD")?;
    if !head.is_branch() {
        bail!("repository is in detached HEAD state");
    }

    head.shorthand()
        .map(str::to_owned)
        .context("failed to resolve branch shorthand")
}

pub fn checkout_existing_branch(repo: &Repository, branch: &str) -> Result<()> {
    ensure_local_branch(repo, branch, false)?;
    checkout_local_branch(repo, branch)
}

pub fn checkout_force_branch(repo: &Repository, branch: &str) -> Result<()> {
    ensure_local_branch(repo, branch, false)?;
    checkout_local_branch(repo, branch)
}

pub fn checkout_diverted_branch(
    repo: &Repository,
    requested_branch: &str,
    diverted_branch: &str,
) -> Result<()> {
    if repo
        .find_branch(diverted_branch, BranchType::Local)
        .is_err()
    {
        let start_point = resolve_branch_target(repo, requested_branch, true)?;
        let commit = repo.find_commit(start_point).with_context(|| {
            format!("failed to load commit `{start_point}` for diverted branch")
        })?;
        repo.branch(diverted_branch, &commit, false)
            .with_context(|| format!("failed to create diverted branch `{diverted_branch}`"))?;
    }

    checkout_local_branch(repo, diverted_branch)
}

fn ensure_local_branch(repo: &Repository, branch: &str, allow_head_fallback: bool) -> Result<()> {
    if repo.find_branch(branch, BranchType::Local).is_ok() {
        return Ok(());
    }

    let target = resolve_branch_target(repo, branch, allow_head_fallback)?;
    let commit = repo
        .find_commit(target)
        .with_context(|| format!("failed to load commit `{target}` for branch `{branch}`"))?;
    repo.branch(branch, &commit, false)
        .with_context(|| format!("failed to create local branch `{branch}`"))?;

    if repo
        .find_branch(&format!("origin/{branch}"), BranchType::Remote)
        .is_ok()
        && let Ok(mut local_branch) = repo.find_branch(branch, BranchType::Local)
    {
        let _ = local_branch.set_upstream(Some(&format!("origin/{branch}")));
    }

    Ok(())
}

fn resolve_branch_target(
    repo: &Repository,
    branch: &str,
    allow_head_fallback: bool,
) -> Result<git2::Oid> {
    if let Ok(local_branch) = repo.find_branch(branch, BranchType::Local)
        && let Some(target) = local_branch.get().target()
    {
        return Ok(target);
    }

    if let Ok(remote_branch) = repo.find_branch(&format!("origin/{branch}"), BranchType::Remote)
        && let Some(target) = remote_branch.get().target()
    {
        return Ok(target);
    }

    if allow_head_fallback {
        // When the requested branch is occupied but does not exist locally or on `origin`, we
        // still create the diverted branch from the current HEAD. This keeps the agent unblocked
        // while making the fallback explicit in code rather than silently inventing a hidden base.
        return head_commit(repo).map(|commit| commit.id());
    }

    bail!("branch `{branch}` was not found locally or on `origin`")
}

fn checkout_local_branch(repo: &Repository, branch: &str) -> Result<()> {
    let reference_name = format!("refs/heads/{branch}");
    repo.set_head(&reference_name)
        .with_context(|| format!("failed to update HEAD to `{reference_name}`"))?;

    let branch_object = repo
        .revparse_single(&reference_name)
        .with_context(|| format!("failed to resolve `{reference_name}`"))?;
    let mut checkout = CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_tree(&branch_object, Some(&mut checkout))
        .with_context(|| format!("failed to check out branch `{branch}`"))?;

    Ok(())
}

fn head_commit(repo: &Repository) -> Result<Commit<'_>> {
    let head = repo.head().context("failed to resolve HEAD")?;
    let head_object = head
        .peel(ObjectType::Commit)
        .context("failed to peel HEAD to commit")?;
    head_object
        .into_commit()
        .map_err(|_| anyhow::anyhow!("HEAD does not point to a commit"))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use anyhow::Result;
    use git2::{Repository, Signature};
    use tempfile::TempDir;

    use super::{checkout_diverted_branch, checkout_existing_branch, current_branch};

    #[test]
    fn checkout_existing_local_branch_moves_head() -> Result<()> {
        let tempdir = TempDir::new()?;
        let repo = init_repo(tempdir.path())?;
        let head_commit = repo.head()?.peel_to_commit()?;
        repo.branch("feature-login", &head_commit, false)?;

        checkout_existing_branch(&repo, "feature-login")?;
        assert_eq!(current_branch(&repo)?, "feature-login");
        Ok(())
    }

    #[test]
    fn diverted_checkout_creates_scoped_branch_from_source() -> Result<()> {
        let tempdir = TempDir::new()?;
        let repo = init_repo(tempdir.path())?;
        let head_commit = repo.head()?.peel_to_commit()?;
        repo.branch("feature-login", &head_commit, false)?;

        checkout_diverted_branch(&repo, "feature-login", "feature-login--agent-a")?;

        assert_eq!(current_branch(&repo)?, "feature-login--agent-a");
        assert!(
            repo.find_branch("feature-login--agent-a", git2::BranchType::Local)
                .is_ok()
        );
        Ok(())
    }

    fn init_repo(path: &Path) -> Result<Repository> {
        let repo = Repository::init(path)?;
        fs::write(path.join("README.md"), "hello\n")?;

        let mut index = repo.index()?;
        index.add_path(Path::new("README.md"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let signature = Signature::now("garc", "garc@example.com")?;
        repo.commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])?;
        drop(tree);
        Ok(repo)
    }
}
