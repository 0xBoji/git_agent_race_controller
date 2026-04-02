use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

const HOOK_FILE_NAME: &str = "post-checkout";
const MANAGED_START: &str = "# >>> garc managed start >>>";
const MANAGED_END: &str = "# <<< garc managed end <<<";
const HOOK_SNIPPET: &str = "# >>> garc managed start >>>\n\
if command -v camp >/dev/null 2>&1; then\n\
  camp update --branch \"$(git rev-parse --abbrev-ref HEAD)\" >/dev/null 2>&1 || true\n\
fi\n\
# <<< garc managed end <<<\n";

pub fn install_post_checkout_hook(git_dir: &Path) -> Result<PathBuf> {
    let hooks_dir = git_dir.join("hooks");
    fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("failed to create hooks directory `{}`", hooks_dir.display()))?;

    let hook_path = hooks_dir.join(HOOK_FILE_NAME);
    let existing = fs::read_to_string(&hook_path).unwrap_or_default();
    let updated = merge_hook_contents(&existing);

    fs::write(&hook_path, updated)
        .with_context(|| format!("failed to write hook `{}`", hook_path.display()))?;
    ensure_executable(&hook_path)?;
    Ok(hook_path)
}

fn merge_hook_contents(existing: &str) -> String {
    let normalized = if existing.trim().is_empty() {
        "#!/bin/sh\n".to_owned()
    } else if existing.starts_with("#!") {
        existing.to_owned()
    } else {
        format!("#!/bin/sh\n\n{}", existing.trim_start())
    };

    // The managed marker block makes `garc init` idempotent: reruns replace only the GARC-owned
    // section while preserving any unrelated user logic that already exists in the hook.
    replace_marked_block(&normalized, MANAGED_START, MANAGED_END, HOOK_SNIPPET)
}

fn replace_marked_block(existing: &str, start: &str, end: &str, replacement: &str) -> String {
    let trimmed_replacement = replacement.trim_end();

    match (existing.find(start), existing.find(end)) {
        (Some(start_idx), Some(end_idx)) if end_idx >= start_idx => {
            let before = existing[..start_idx].trim_end();
            let after = existing[end_idx + end.len()..].trim_start();
            if before.is_empty() && after.is_empty() {
                format!("{trimmed_replacement}\n")
            } else if before.is_empty() {
                format!("{trimmed_replacement}\n\n{after}\n")
            } else if after.is_empty() {
                format!("{before}\n\n{trimmed_replacement}\n")
            } else {
                format!("{before}\n\n{trimmed_replacement}\n\n{after}\n")
            }
        }
        _ if existing.trim().is_empty() => format!("{trimmed_replacement}\n"),
        _ => format!("{}\n\n{trimmed_replacement}\n", existing.trim_end()),
    }
}

#[cfg(unix)]
fn ensure_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to stat hook `{}`", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to chmod hook `{}`", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Result;
    use tempfile::TempDir;

    use super::{HOOK_FILE_NAME, HOOK_SNIPPET, install_post_checkout_hook};

    #[test]
    fn installer_is_idempotent_and_preserves_existing_content() -> Result<()> {
        let tempdir = TempDir::new()?;
        let git_dir = tempdir.path().join(".git");
        let hooks_dir = git_dir.join("hooks");
        fs::create_dir_all(&hooks_dir)?;

        let hook_path = hooks_dir.join(HOOK_FILE_NAME);
        fs::write(&hook_path, "#!/bin/sh\n\necho custom\n")?;

        install_post_checkout_hook(&git_dir)?;
        install_post_checkout_hook(&git_dir)?;

        let contents = fs::read_to_string(&hook_path)?;
        assert!(contents.contains("echo custom"));
        assert_eq!(contents.matches(HOOK_SNIPPET.trim()).count(), 1);
        Ok(())
    }
}
