use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeContext {
    pub repo_root: String,
    pub worktree_path: String,
    pub current_branch: String,
    pub parent_branch: String,
    pub agent_name: String,
}

/// Returns the `.worktrees` directory rooted under the canonical repo root.
pub fn worktree_base_dir(repo_root: &str) -> PathBuf {
    PathBuf::from(repo_root).join(WORKTREE_DIR)
}

/// Returns the full path for one named workdir checkout.
pub fn worktree_path(repo_root: &str, agent_name: &str) -> PathBuf {
    worktree_base_dir(repo_root).join(agent_name)
}

pub fn worktree_slug(label: &str) -> Result<String, String> {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in label.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_uppercase());
            last_was_dash = false;
            continue;
        }
        if matches!(ch, '-' | '_' | ' ' | '.' | '/') && !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        return Err("workdir slug must contain at least one ASCII letter or digit".to_owned());
    }
    Ok(slug)
}

/// Returns the branch name used for one workdir clone.
pub fn worktree_branch(agent_name: &str) -> String {
    format!("{WORKTREE_BRANCH_PREFIX}{agent_name}")
}

pub fn worktree_agent_name(branch: &str) -> Option<&str> {
    branch
        .strip_prefix(WORKTREE_BRANCH_PREFIX)
        .filter(|name| !name.is_empty())
}

pub fn worktree_current_path(cwd: &str) -> Result<String, String> {
    get_repo_root(cwd)
}

pub fn is_linked_worktree(cwd: &str) -> Result<bool, String> {
    let git_dir = resolve_git_path(
        cwd,
        &run_git_capture(cwd, &["rev-parse", "--git-dir"], GIT_TIMEOUT_SECS)?,
    )?;
    let common_dir = resolve_git_path(
        cwd,
        &run_git_capture(cwd, &["rev-parse", "--git-common-dir"], GIT_TIMEOUT_SECS)?,
    )?;
    Ok(git_dir != common_dir)
}

pub fn resolve_parent_branch(
    repo_root: &str,
    override_branch: Option<&str>,
) -> Result<String, String> {
    if let Some(branch) = override_branch
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return branch_exists(repo_root, branch)?
            .then(|| branch.to_owned())
            .ok_or_else(|| format!("parent branch `{branch}` does not exist"));
    }

    for branch in ["main", "master"] {
        if branch_exists(repo_root, branch)? {
            return Ok(branch.to_owned());
        }
    }

    Err("failed to resolve parent branch: neither `main` nor `master` exists".to_owned())
}

pub fn resolve_worktree_context(
    cwd: &str,
    parent_branch_override: Option<&str>,
) -> Result<WorktreeContext, String> {
    let repo_root = get_worktree_repo_root(cwd)?;
    if !is_linked_worktree(cwd)? {
        return Err("worktree lifecycle commands require a linked worktree checkout".to_owned());
    }

    let worktree_path = worktree_current_path(cwd)?;
    let current_branch = get_current_branch(cwd)?;
    if current_branch == "HEAD" {
        return Err(
            "worktree lifecycle commands do not support detached HEAD checkouts".to_owned(),
        );
    }
    let agent_name = worktree_agent_name(&current_branch)
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "worktree lifecycle commands require a `{WORKTREE_BRANCH_PREFIX}*` branch; got `{current_branch}`"
            )
        })?;
    let parent_branch = resolve_parent_branch(&repo_root, parent_branch_override)?;

    Ok(WorktreeContext {
        repo_root,
        worktree_path,
        current_branch,
        parent_branch,
        agent_name,
    })
}

/// Resolves the canonical/common repository root for a checkout path.
pub fn get_worktree_repo_root(cwd: &str) -> Result<String, String> {
    let common_dir = run_git_capture(cwd, &["rev-parse", "--git-common-dir"], GIT_TIMEOUT_SECS)?;
    let common_dir = resolve_git_path(cwd, &common_dir)?;

    if Path::new(&common_dir)
        .file_name()
        .and_then(|value| value.to_str())
        == Some(".git")
    {
        return Ok(Path::new(&common_dir)
            .parent()
            .ok_or_else(|| {
                format!(
                    "failed to resolve repo root from `{}`",
                    common_dir.display()
                )
            })?
            .to_string_lossy()
            .into_owned());
    }

    get_repo_root(cwd)
}

/// Returns the checkout root for one git directory.
pub fn get_repo_root(cwd: &str) -> Result<String, String> {
    run_git_capture(cwd, &["rev-parse", "--show-toplevel"], GIT_TIMEOUT_SECS)
}

/// Returns the current branch name for one git checkout.
pub fn get_current_branch(cwd: &str) -> Result<String, String> {
    run_git_capture(
        cwd,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        GIT_TIMEOUT_SECS,
    )
}
