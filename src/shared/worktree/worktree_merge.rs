use super::*;

pub fn ensure_clean_worktree(cwd: &str) -> Result<(), String> {
    let status = run_git_capture(cwd, &["status", "--porcelain"], GIT_TIMEOUT_SECS)?;
    if status.trim().is_empty() {
        Ok(())
    } else {
        Err(format!("git checkout is dirty:\n{status}"))
    }
}

pub fn merge_worktree_into_parent(ctx: &WorktreeContext) -> Result<String, String> {
    checkout_branch(&ctx.repo_root, &ctx.parent_branch)?;
    merge_with_cleanup(
        &ctx.repo_root,
        &[
            "merge",
            "--no-ff",
            &ctx.current_branch,
            "-m",
            &format!("Merge {} into {}", ctx.current_branch, ctx.parent_branch),
        ],
        &format!("merge {} into {}", ctx.current_branch, ctx.parent_branch),
    )
}

pub fn merge_parent_back_into_worktree(ctx: &WorktreeContext) -> Result<String, String> {
    checkout_branch(&ctx.worktree_path, &ctx.current_branch)?;
    merge_with_cleanup(
        &ctx.worktree_path,
        &[
            "merge",
            &ctx.parent_branch,
            "-m",
            &format!(
                "Merge {} back into {}",
                ctx.parent_branch, ctx.current_branch
            ),
        ],
        &format!("merge {} into {}", ctx.parent_branch, ctx.current_branch),
    )
}

pub fn conflicted_files(cwd: &str) -> Result<Vec<String>, String> {
    let files = run_git_capture(
        cwd,
        &["diff", "--name-only", "--diff-filter=U"],
        GIT_TIMEOUT_SECS,
    )?;
    let mut files = files
        .lines()
        .map(str::trim)
        .filter(|line: &&str| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    Ok(files)
}

pub fn abort_merge(cwd: &str) -> Result<(), String> {
    if !merge_head_exists(cwd)? {
        return Ok(());
    }
    run_git_capture(cwd, &["merge", "--abort"], GIT_TIMEOUT_SECS)?;
    Ok(())
}

fn checkout_branch(cwd: &str, branch: &str) -> Result<(), String> {
    run_git_capture(cwd, &["checkout", branch], GIT_LIFECYCLE_TIMEOUT_SECS).map(|_| ())
}

fn merge_with_cleanup(cwd: &str, args: &[&str], description: &str) -> Result<String, String> {
    match run_git_capture(cwd, args, GIT_LIFECYCLE_TIMEOUT_SECS) {
        Ok(output) => Ok(output),
        Err(error) => {
            let conflicts = conflicted_files(cwd)?;
            abort_merge(cwd)?;
            if conflicts.is_empty() {
                Err(format!("failed to {description}: {error}"))
            } else {
                Err(format!(
                    "failed to {description}; conflicted files: {}",
                    conflicts.join(", ")
                ))
            }
        }
    }
}
