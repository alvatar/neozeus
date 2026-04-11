use super::*;

pub fn remove_worktree_and_branch(ctx: &WorktreeContext) -> Result<(), String> {
    let mut errors = Vec::new();

    if let Err(error) = run_git_capture(
        &ctx.repo_root,
        &["worktree", "remove", "--force", &ctx.worktree_path],
        GIT_LIFECYCLE_TIMEOUT_SECS,
    ) {
        errors.push(format!(
            "failed to remove worktree `{}`: {error}",
            ctx.worktree_path
        ));
    }

    let _ = run_git_capture(
        &ctx.repo_root,
        &["worktree", "prune"],
        GIT_LIFECYCLE_TIMEOUT_SECS,
    );

    if let Err(error) = run_git_capture(
        &ctx.repo_root,
        &["branch", "-D", &ctx.current_branch],
        GIT_LIFECYCLE_TIMEOUT_SECS,
    ) {
        errors.push(format!(
            "failed to delete branch `{}`: {error}",
            ctx.current_branch
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Creates one worktree checkout and branch under the canonical repo root.
pub fn create_worktree(
    repo_root: &str,
    agent_name: &str,
    base_branch: Option<&str>,
) -> Result<String, String> {
    let repo_root = repo_root.trim();
    if repo_root.is_empty() {
        return Err("worktree repo root must not be empty".to_owned());
    }

    let worktree_path = worktree_path(repo_root, agent_name);
    if worktree_path.exists() {
        return Err(format!(
            "Worktree path already exists: {}",
            worktree_path.display()
        ));
    }

    ensure_gitignore_entry(repo_root)?;
    fs::create_dir_all(worktree_base_dir(repo_root)).map_err(|error| {
        format!(
            "failed to create worktree parent directory {}: {error}",
            worktree_base_dir(repo_root).display()
        )
    })?;

    let mut command = prepare_git_command(repo_root);
    command.arg("worktree");
    command.arg("add");
    command.arg("-b");
    command.arg(worktree_branch(agent_name));
    command.arg(&worktree_path);
    if let Some(base_branch) = base_branch.map(str::trim).filter(|value| !value.is_empty()) {
        command.arg(base_branch);
    }
    let output = command.output().map_err(|error| {
        format!(
            "failed to execute `git worktree add` in {}: {error}",
            repo_root
        )
    })?;
    if !output.status.success() {
        return Err(format!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(worktree_path.to_string_lossy().into_owned())
}

fn ensure_gitignore_entry(repo_root: &str) -> Result<(), String> {
    let gitignore_path = Path::new(repo_root).join(".gitignore");
    let entry = format!("/{WORKTREE_DIR}/");
    let existing = match fs::read_to_string(&gitignore_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(format!(
                "failed to read gitignore {}: {error}",
                gitignore_path.display()
            ))
        }
    };
    if existing.lines().any(|line| line.trim() == entry) {
        return Ok(());
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&entry);
    updated.push('\n');
    fs::write(&gitignore_path, updated).map_err(|error| {
        format!(
            "failed to write gitignore {}: {error}",
            gitignore_path.display()
        )
    })
}
