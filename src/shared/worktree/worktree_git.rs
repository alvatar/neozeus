use super::*;

pub(super) fn prepare_git_command(cwd: &str) -> Command {
    let mut command = Command::new(git_program());
    command.current_dir(cwd);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
    command
}

#[cfg(not(test))]
fn git_program() -> &'static str {
    "git"
}

#[cfg(test)]
fn git_program() -> &'static str {
    "git"
}

pub(super) fn branch_exists(cwd: &str, branch: &str) -> Result<bool, String> {
    let output = run_git_output(
        cwd,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
        GIT_TIMEOUT_SECS,
    )?;
    Ok(output.status.success())
}

pub(super) fn merge_head_exists(cwd: &str) -> Result<bool, String> {
    let merge_head = run_git_capture(
        cwd,
        &["rev-parse", "--git-path", "MERGE_HEAD"],
        GIT_TIMEOUT_SECS,
    )?;
    Ok(resolve_git_path(cwd, &merge_head)?.exists())
}

pub(super) fn resolve_git_path(cwd: &str, value: &str) -> Result<PathBuf, String> {
    let path = if Path::new(value).is_absolute() {
        PathBuf::from(value)
    } else {
        Path::new(cwd).join(value)
    };
    Ok(path.canonicalize().unwrap_or(path))
}

pub(super) fn run_git_output(
    cwd: &str,
    args: &[&str],
    timeout_secs: u64,
) -> Result<CommandOutput, String> {
    let mut command = prepare_git_command(cwd);
    command.args(args);
    run_command_with_timeout(&mut command, Duration::from_secs(timeout_secs), true)
        .map_err(|error| format!("failed to execute git {:?} in {}: {error}", args, cwd))
}

pub(super) fn run_git_capture(
    cwd: &str,
    args: &[&str],
    timeout_secs: u64,
) -> Result<String, String> {
    let output = run_git_output(cwd, args, timeout_secs)?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} failed in {}: {}",
            args,
            cwd,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
