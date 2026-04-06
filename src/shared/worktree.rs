use crate::shared::command_runner::{run_command_with_timeout, CommandOutput};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const GIT_TIMEOUT_SECS: u64 = 5;
const GIT_LIFECYCLE_TIMEOUT_SECS: u64 = 30;
const WORKTREE_DIR: &str = ".worktrees";
const WORKTREE_BRANCH_PREFIX: &str = "neozeus/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeContext {
    pub repo_root: String,
    pub worktree_path: String,
    pub current_branch: String,
    pub parent_branch: String,
    pub agent_name: String,
}

fn prepare_git_command(cwd: &str) -> Command {
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

/// Returns the `.worktrees` directory rooted under the canonical repo root.
pub fn worktree_base_dir(repo_root: &str) -> PathBuf {
    PathBuf::from(repo_root).join(WORKTREE_DIR)
}

/// Returns the full path for one named workdir checkout.
pub fn worktree_path(repo_root: &str, agent_name: &str) -> PathBuf {
    worktree_base_dir(repo_root).join(agent_name)
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
        .filter(|line| !line.is_empty())
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

fn branch_exists(cwd: &str, branch: &str) -> Result<bool, String> {
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

fn merge_head_exists(cwd: &str) -> Result<bool, String> {
    let merge_head = run_git_capture(
        cwd,
        &["rev-parse", "--git-path", "MERGE_HEAD"],
        GIT_TIMEOUT_SECS,
    )?;
    Ok(resolve_git_path(cwd, &merge_head)?.exists())
}

fn resolve_git_path(cwd: &str, value: &str) -> Result<PathBuf, String> {
    let path = if Path::new(value).is_absolute() {
        PathBuf::from(value)
    } else {
        Path::new(cwd).join(value)
    };
    Ok(path.canonicalize().unwrap_or(path))
}

fn run_git_output(cwd: &str, args: &[&str], timeout_secs: u64) -> Result<CommandOutput, String> {
    let mut command = prepare_git_command(cwd);
    command.args(args);
    run_command_with_timeout(&mut command, Duration::from_secs(timeout_secs), true)
        .map_err(|error| format!("failed to execute git {:?} in {}: {error}", args, cwd))
}

fn run_git_capture(cwd: &str, args: &[&str], timeout_secs: u64) -> Result<String, String> {
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

#[cfg(test)]
mod tests {
    use super::{
        abort_merge, branch_exists, conflicted_files, create_worktree, ensure_clean_worktree,
        get_current_branch, get_repo_root, get_worktree_repo_root, is_linked_worktree,
        merge_parent_back_into_worktree, merge_worktree_into_parent, remove_worktree_and_branch,
        resolve_parent_branch, resolve_worktree_context, worktree_agent_name, worktree_base_dir,
        worktree_branch, worktree_current_path, worktree_path, WorktreeContext,
    };
    use std::{
        path::{Path, PathBuf},
        process::Command,
        sync::OnceLock,
    };

    fn temp_dir(prefix: &str) -> PathBuf {
        static COUNTER: OnceLock<std::sync::atomic::AtomicU64> = OnceLock::new();
        let counter = COUNTER.get_or_init(|| std::sync::atomic::AtomicU64::new(0));
        let path = PathBuf::from("/tmp").join(format!(
            "neozeus-{prefix}-{}-{}",
            std::process::id(),
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("temp dir should create");
        path
    }

    fn run(repo_root: &Path, args: &[&str]) {
        let output = Command::new(args[0])
            .current_dir(repo_root)
            .args(&args[1..])
            .output()
            .expect("command should run");
        assert!(
            output.status.success(),
            "command {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_text(repo_root: &Path, args: &[&str]) -> String {
        let output = Command::new(args[0])
            .current_dir(repo_root)
            .args(&args[1..])
            .output()
            .expect("command should run");
        assert!(
            output.status.success(),
            "command {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn run_allow_failure(repo_root: &Path, args: &[&str]) -> std::process::Output {
        Command::new(args[0])
            .current_dir(repo_root)
            .args(&args[1..])
            .output()
            .expect("command should run")
    }

    fn init_git_repo() -> PathBuf {
        init_git_repo_on_branch("main")
    }

    fn init_git_repo_on_branch(default_branch: &str) -> PathBuf {
        let repo = temp_dir("worktree-repo");
        run(&repo, &["git", "init"]);
        run(
            &repo,
            &["git", "config", "user.email", "neozeus@example.test"],
        );
        run(&repo, &["git", "config", "user.name", "NeoZeus Test"]);
        std::fs::write(repo.join("README.md"), "seed\n").unwrap();
        std::fs::write(repo.join(".gitignore"), "/.worktrees/\n").unwrap();
        run(&repo, &["git", "add", "README.md", ".gitignore"]);
        run(&repo, &["git", "commit", "-m", "initial"]);
        run(&repo, &["git", "branch", "-M", default_branch]);
        repo
    }

    fn add_manual_worktree(
        repo: &Path,
        checkout_name: &str,
        branch_name: &str,
        base_branch: &str,
    ) -> PathBuf {
        let path = repo.join(".worktrees").join(checkout_name);
        std::fs::create_dir_all(repo.join(".worktrees")).unwrap();
        run(
            repo,
            &[
                "git",
                "worktree",
                "add",
                "-b",
                branch_name,
                path.to_str().unwrap(),
                base_branch,
            ],
        );
        path
    }

    fn write_and_commit(repo: &Path, file: &str, contents: &str, message: &str) {
        std::fs::write(repo.join(file), contents).unwrap();
        run(repo, &["git", "add", file]);
        run(repo, &["git", "commit", "-m", message]);
    }

    fn worktree_context(repo: &Path, worktree: &Path) -> WorktreeContext {
        resolve_worktree_context(worktree.to_str().unwrap(), None).unwrap_or_else(|error| {
            panic!(
                "failed to resolve worktree context for {} in {}: {error}",
                worktree.display(),
                repo.display()
            )
        })
    }

    fn branch_tip(repo: &Path, branch: &str) -> String {
        run_text(repo, &["git", "rev-parse", branch])
    }

    fn head_tip(repo: &Path) -> String {
        run_text(repo, &["git", "rev-parse", "HEAD"])
    }

    fn assert_clean(repo: &Path) {
        let status = run_text(repo, &["git", "status", "--porcelain"]);
        assert!(
            status.is_empty(),
            "expected clean repo {}, got `{status}`",
            repo.display()
        );
    }

    fn assert_branch_contains(repo: &Path, ancestor: &str, descendant: &str) {
        let output = run_allow_failure(
            repo,
            &["git", "merge-base", "--is-ancestor", ancestor, descendant],
        );
        assert!(
            output.status.success(),
            "expected {descendant} to contain {ancestor}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn merge_head_exists(repo: &Path) -> bool {
        repo.join(".git").join("MERGE_HEAD").exists() || {
            let merge_head_path = run_text(repo, &["git", "rev-parse", "--git-path", "MERGE_HEAD"]);
            PathBuf::from(merge_head_path).exists()
        }
    }

    fn collect_source_files(root: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_source_files(&path, files);
            } else if matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("rs" | "sh")
            ) {
                files.push(path);
            }
        }
    }

    fn has_legacy_worktree_prefix(text: &str) -> bool {
        text.match_indices("zeus/").any(|(index, _)| {
            !text[..index]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
    }

    #[test]
    fn worktree_path_and_branch_follow_neozeus_layout() {
        let repo_root = "/tmp/project";
        assert_eq!(
            worktree_base_dir(repo_root),
            PathBuf::from("/tmp/project/.worktrees")
        );
        assert_eq!(
            worktree_path(repo_root, "alpha"),
            PathBuf::from("/tmp/project/.worktrees/alpha")
        );
        assert_eq!(worktree_branch("alpha"), "neozeus/alpha");
    }

    #[test]
    fn worktree_agent_name_extracts_neozeus_suffix() {
        assert_eq!(worktree_agent_name("neozeus/alpha"), Some("alpha"));
        assert_eq!(worktree_agent_name("neozeus/"), None);
        assert_eq!(worktree_agent_name("feature/alpha"), None);
    }

    #[test]
    fn get_worktree_repo_root_resolves_main_checkout_root() {
        let repo = init_git_repo();
        assert_eq!(
            get_repo_root(repo.to_str().unwrap()).unwrap(),
            repo.to_string_lossy()
        );
        assert_eq!(
            get_worktree_repo_root(repo.to_str().unwrap()).unwrap(),
            repo.to_string_lossy()
        );
        assert_eq!(get_current_branch(repo.to_str().unwrap()).unwrap(), "main");
        assert!(!is_linked_worktree(repo.to_str().unwrap()).unwrap());
    }

    #[test]
    fn get_worktree_repo_root_from_linked_worktree_returns_common_root() {
        let repo = init_git_repo();
        let worktree = create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap();
        let nested = PathBuf::from(&worktree).join("src");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            get_worktree_repo_root(nested.to_str().unwrap()).unwrap(),
            repo.to_string_lossy()
        );
        assert!(is_linked_worktree(nested.to_str().unwrap()).unwrap());
    }

    #[test]
    fn create_worktree_creates_real_checkout_branch_and_gitignore_entry() {
        let repo = init_git_repo();
        let worktree = create_worktree(repo.to_str().unwrap(), "beta", Some("main")).unwrap();
        let worktree = PathBuf::from(worktree);

        assert!(worktree.is_dir());
        assert_eq!(
            get_current_branch(worktree.to_str().unwrap()).unwrap(),
            "neozeus/beta"
        );
        let gitignore = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert_eq!(
            gitignore
                .lines()
                .filter(|line| line.trim() == "/.worktrees/")
                .count(),
            1
        );
    }

    #[test]
    fn create_worktree_from_linked_checkout_uses_common_repo_root() {
        let repo = init_git_repo();
        let alpha =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        let common_root = get_worktree_repo_root(alpha.to_str().unwrap()).unwrap();
        let beta = create_worktree(&common_root, "beta", Some("main")).unwrap();
        assert_eq!(PathBuf::from(beta), repo.join(".worktrees").join("beta"));
        assert_eq!(
            get_current_branch(repo.join(".worktrees/beta").to_str().unwrap()).unwrap(),
            "neozeus/beta"
        );
    }

    #[test]
    fn create_worktree_rejects_duplicate_paths() {
        let repo = init_git_repo();
        create_worktree(repo.to_str().unwrap(), "dup", Some("main")).unwrap();
        let error = create_worktree(repo.to_str().unwrap(), "dup", Some("main"))
            .expect_err("duplicate worktree should fail");
        assert!(error.contains("Worktree path already exists"));
    }

    #[test]
    fn get_worktree_repo_root_rejects_non_git_source() {
        let dir = temp_dir("worktree-non-git");
        let error =
            get_worktree_repo_root(dir.to_str().unwrap()).expect_err("non-git cwd should fail");
        assert!(error.contains("git"));
    }

    #[test]
    fn worktree_current_path_returns_checkout_root() {
        let repo = init_git_repo();
        let worktree = create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap();
        let nested = PathBuf::from(&worktree).join("nested/deeper");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            worktree_current_path(nested.to_str().unwrap()).unwrap(),
            worktree
        );
    }

    #[test]
    fn resolve_parent_branch_prefers_explicit_override() {
        let repo = init_git_repo();
        run(&repo, &["git", "branch", "release"]);
        assert_eq!(
            resolve_parent_branch(repo.to_str().unwrap(), Some(" release ")).unwrap(),
            "release"
        );
    }

    #[test]
    fn resolve_parent_branch_falls_back_to_main_then_master() {
        let main_repo = init_git_repo_on_branch("main");
        assert_eq!(
            resolve_parent_branch(main_repo.to_str().unwrap(), None).unwrap(),
            "main"
        );

        let master_repo = init_git_repo_on_branch("master");
        assert_eq!(
            resolve_parent_branch(master_repo.to_str().unwrap(), None).unwrap(),
            "master"
        );
    }

    #[test]
    fn resolve_parent_branch_errors_when_target_is_missing() {
        let repo = init_git_repo_on_branch("trunk");
        let error = resolve_parent_branch(repo.to_str().unwrap(), None)
            .expect_err("missing parent branch should fail");
        assert!(error.contains("neither `main` nor `master` exists"));
    }

    #[test]
    fn resolve_parent_branch_errors_when_override_is_missing() {
        let repo = init_git_repo();
        let error = resolve_parent_branch(repo.to_str().unwrap(), Some("release"))
            .expect_err("missing override should fail");
        assert_eq!(error, "parent branch `release` does not exist");
    }

    #[test]
    fn resolve_worktree_context_succeeds_from_linked_checkout_root() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());

        let context = resolve_worktree_context(worktree.to_str().unwrap(), None).unwrap();
        assert_eq!(context.repo_root, repo.to_string_lossy());
        assert_eq!(context.worktree_path, worktree.to_string_lossy());
        assert_eq!(context.current_branch, "neozeus/alpha");
        assert_eq!(context.parent_branch, "main");
        assert_eq!(context.agent_name, "alpha");
    }

    #[test]
    fn resolve_worktree_context_succeeds_from_nested_linked_path() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        let nested = worktree.join("nested/deeper");
        std::fs::create_dir_all(&nested).unwrap();

        let context = resolve_worktree_context(nested.to_str().unwrap(), None).unwrap();
        assert_eq!(context.worktree_path, worktree.to_string_lossy());
        assert_eq!(context.agent_name, "alpha");
    }

    #[test]
    fn resolve_worktree_context_rejects_main_checkout() {
        let repo = init_git_repo();
        let error = resolve_worktree_context(repo.to_str().unwrap(), None)
            .expect_err("main checkout should fail");
        assert!(error.contains("linked worktree checkout"));
    }

    #[test]
    fn resolve_worktree_context_rejects_non_git_cwd() {
        let dir = temp_dir("worktree-context-non-git");
        let error = resolve_worktree_context(dir.to_str().unwrap(), None)
            .expect_err("non-git cwd should fail");
        assert!(error.contains("git"));
    }

    #[test]
    fn resolve_worktree_context_rejects_detached_head() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        run(&worktree, &["git", "checkout", "HEAD~0"]);

        let error = resolve_worktree_context(worktree.to_str().unwrap(), None)
            .expect_err("detached head should fail");
        assert!(error.contains("detached HEAD"));
    }

    #[test]
    fn resolve_worktree_context_rejects_non_neozeus_branch() {
        let repo = init_git_repo();
        let worktree = add_manual_worktree(&repo, "plain", "feature/plain", "main");

        let error = resolve_worktree_context(worktree.to_str().unwrap(), None)
            .expect_err("non-neozeus branch should fail");
        assert!(error.contains("neozeus/"));
    }

    #[test]
    fn resolve_worktree_context_uses_explicit_parent_override() {
        let repo = init_git_repo();
        run(&repo, &["git", "branch", "release"]);
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());

        let context =
            resolve_worktree_context(worktree.to_str().unwrap(), Some("release")).unwrap();
        assert_eq!(context.parent_branch, "release");
    }

    #[test]
    fn ensure_clean_worktree_accepts_clean_checkout() {
        let repo = init_git_repo();
        assert!(ensure_clean_worktree(repo.to_str().unwrap()).is_ok());
    }

    #[test]
    fn ensure_clean_worktree_rejects_modified_tracked_file() {
        let repo = init_git_repo();
        std::fs::write(repo.join("README.md"), "changed\n").unwrap();
        let error =
            ensure_clean_worktree(repo.to_str().unwrap()).expect_err("modified file should fail");
        assert!(error.contains("README.md"));
    }

    #[test]
    fn ensure_clean_worktree_rejects_staged_only_change() {
        let repo = init_git_repo();
        std::fs::write(repo.join("README.md"), "changed\n").unwrap();
        run(&repo, &["git", "add", "README.md"]);
        let error =
            ensure_clean_worktree(repo.to_str().unwrap()).expect_err("staged change should fail");
        assert!(error.contains("README.md"));
    }

    #[test]
    fn ensure_clean_worktree_rejects_staged_and_unstaged_changes() {
        let repo = init_git_repo();
        std::fs::write(repo.join("README.md"), "staged\n").unwrap();
        run(&repo, &["git", "add", "README.md"]);
        std::fs::write(repo.join("README.md"), "staged-and-unstaged\n").unwrap();
        let error = ensure_clean_worktree(repo.to_str().unwrap())
            .expect_err("mixed dirty state should fail");
        assert!(error.contains("MM README.md") || error.contains("README.md"));
    }

    #[test]
    fn ensure_clean_worktree_rejects_untracked_files() {
        let repo = init_git_repo();
        std::fs::write(repo.join("UNTRACKED.txt"), "x\n").unwrap();
        let error =
            ensure_clean_worktree(repo.to_str().unwrap()).expect_err("untracked file should fail");
        assert!(error.contains("UNTRACKED.txt"));
    }

    #[test]
    fn ensure_clean_worktree_rejects_deleted_tracked_files() {
        let repo = init_git_repo();
        std::fs::remove_file(repo.join("README.md")).unwrap();
        let error = ensure_clean_worktree(repo.to_str().unwrap())
            .expect_err("deleted tracked file should fail");
        assert!(error.contains("README.md"));
    }

    #[test]
    fn ensure_clean_worktree_rejects_rename_status() {
        let repo = init_git_repo();
        run(&repo, &["git", "mv", "README.md", "RENAMED.md"]);
        let error = ensure_clean_worktree(repo.to_str().unwrap()).expect_err("rename should fail");
        assert!(error.contains("RENAMED.md") || error.contains("README.md"));
    }

    #[test]
    fn merge_worktree_into_parent_creates_merge_commit() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        write_and_commit(&worktree, "feature.txt", "alpha\n", "feature");
        let ctx = worktree_context(&repo, &worktree);

        let _output = merge_worktree_into_parent(&ctx).unwrap();

        assert_eq!(
            run_text(&repo, &["git", "show", "HEAD:feature.txt"]),
            "alpha"
        );
        assert_eq!(get_current_branch(repo.to_str().unwrap()).unwrap(), "main");
        assert_eq!(branch_tip(&repo, "main"), head_tip(&repo));
        assert!(branch_tip(&repo, "main") != branch_tip(&repo, &ctx.current_branch));
        assert!(!merge_head_exists(&repo));
    }

    #[test]
    fn merge_worktree_into_parent_reports_conflicts_and_aborts_cleanly() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        write_and_commit(&worktree, "README.md", "worktree\n", "worktree change");
        write_and_commit(&repo, "README.md", "main\n", "main change");
        let ctx = worktree_context(&repo, &worktree);

        let error = merge_worktree_into_parent(&ctx).expect_err("conflicting merge should fail");

        assert!(error.contains("README.md"));
        assert!(!merge_head_exists(&repo));
        assert_clean(&repo);
    }

    #[test]
    fn merge_parent_back_into_worktree_updates_branch() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        let ctx = worktree_context(&repo, &worktree);
        write_and_commit(&repo, "main-only.txt", "main\n", "main change");

        merge_parent_back_into_worktree(&ctx).unwrap();

        assert_eq!(
            run_text(&worktree, &["git", "show", "HEAD:main-only.txt"]),
            "main"
        );
        assert!(!merge_head_exists(&worktree));
        assert_clean(&worktree);
    }

    #[test]
    fn merge_parent_back_into_worktree_reports_conflicts_and_aborts_cleanly() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        write_and_commit(&repo, "README.md", "main\n", "main change");
        write_and_commit(&worktree, "README.md", "worktree\n", "worktree change");
        let ctx = worktree_context(&repo, &worktree);

        let error =
            merge_parent_back_into_worktree(&ctx).expect_err("conflicting merge-back should fail");

        assert!(error.contains("README.md"));
        assert!(!merge_head_exists(&worktree));
        assert_clean(&worktree);
    }

    #[test]
    fn merge_continue_end_to_end_updates_parent_and_worktree() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        write_and_commit(&worktree, "feature.txt", "alpha\n", "feature");
        let ctx = worktree_context(&repo, &worktree);

        merge_worktree_into_parent(&ctx).unwrap();
        merge_parent_back_into_worktree(&ctx).unwrap();

        assert_branch_contains(&worktree, "main", "neozeus/alpha");
        assert_clean(&repo);
        assert_clean(&worktree);
    }

    #[test]
    fn merge_continue_second_merge_conflict_aborts_cleanly() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        write_and_commit(&worktree, "README.md", "worktree-base\n", "worktree base");
        let ctx = worktree_context(&repo, &worktree);

        merge_worktree_into_parent(&ctx).unwrap();
        write_and_commit(&repo, "README.md", "main-after-merge\n", "main after merge");
        write_and_commit(
            &worktree,
            "README.md",
            "worktree-after-merge\n",
            "worktree after merge",
        );

        let error =
            merge_parent_back_into_worktree(&ctx).expect_err("merge-back conflict should fail");

        assert!(error.contains("README.md"));
        assert!(!merge_head_exists(&worktree));
        assert_clean(&worktree);
        assert_clean(&repo);
    }

    #[test]
    fn merge_finalize_end_to_end_merges_then_removes_worktree_and_branch() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        write_and_commit(&worktree, "feature.txt", "alpha\n", "feature");
        let ctx = worktree_context(&repo, &worktree);

        merge_worktree_into_parent(&ctx).unwrap();
        remove_worktree_and_branch(&ctx).unwrap();

        assert_eq!(
            run_text(&repo, &["git", "show", "HEAD:feature.txt"]),
            "alpha"
        );
        assert!(!worktree.exists());
        assert!(!branch_exists(repo.to_str().unwrap(), "neozeus/alpha").unwrap());
        assert_clean(&repo);
    }

    #[test]
    fn discard_end_to_end_removes_dirty_worktree_without_merging_parent() {
        let repo = init_git_repo();
        let parent_before = branch_tip(&repo, "main");
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        std::fs::write(worktree.join("dirty.txt"), "uncommitted\n").unwrap();
        let ctx = worktree_context(&repo, &worktree);

        remove_worktree_and_branch(&ctx).unwrap();

        assert_eq!(branch_tip(&repo, "main"), parent_before);
        assert!(!worktree.exists());
        assert!(!branch_exists(repo.to_str().unwrap(), "neozeus/alpha").unwrap());
        assert_clean(&repo);
    }

    #[test]
    fn conflicted_files_returns_exact_paths_for_real_conflicts() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        write_and_commit(&repo, "README.md", "main\n", "main change");
        write_and_commit(&worktree, "README.md", "worktree\n", "worktree change");
        let output = run_allow_failure(&worktree, &["git", "merge", "main"]);
        assert!(!output.status.success(), "merge unexpectedly succeeded");

        let files = conflicted_files(worktree.to_str().unwrap()).unwrap();
        assert_eq!(files, vec!["README.md".to_owned()]);
        abort_merge(worktree.to_str().unwrap()).unwrap();
    }

    #[test]
    fn abort_merge_is_effectively_idempotent() {
        let repo = init_git_repo();
        abort_merge(repo.to_str().unwrap()).unwrap();
    }

    #[test]
    fn remove_worktree_and_branch_removes_checkout_and_branch() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        let ctx = worktree_context(&repo, &worktree);

        remove_worktree_and_branch(&ctx).unwrap();

        assert!(!worktree.exists());
        assert!(!branch_exists(repo.to_str().unwrap(), "neozeus/alpha").unwrap());
    }

    #[test]
    fn remove_worktree_and_branch_reports_worktree_remove_failure() {
        let repo = init_git_repo();
        run(&repo, &["git", "branch", "neozeus/spare"]);
        let ctx = WorktreeContext {
            repo_root: repo.to_string_lossy().into_owned(),
            worktree_path: repo
                .join(".worktrees/missing")
                .to_string_lossy()
                .into_owned(),
            current_branch: "neozeus/spare".into(),
            parent_branch: "main".into(),
            agent_name: "spare".into(),
        };

        let error = remove_worktree_and_branch(&ctx).expect_err("missing worktree should fail");
        assert!(error.contains("failed to remove worktree"));
        assert!(!branch_exists(repo.to_str().unwrap(), "neozeus/spare").unwrap());
    }

    #[test]
    fn remove_worktree_and_branch_reports_branch_delete_failure() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        let ctx = WorktreeContext {
            repo_root: repo.to_string_lossy().into_owned(),
            worktree_path: worktree.to_string_lossy().into_owned(),
            current_branch: "neozeus/missing".into(),
            parent_branch: "main".into(),
            agent_name: "missing".into(),
        };

        let error = remove_worktree_and_branch(&ctx).expect_err("missing branch should fail");
        assert!(error.contains("failed to delete branch"));
        assert!(!worktree.exists());
    }

    #[test]
    fn remove_worktree_and_branch_preserves_both_failures() {
        let repo = init_git_repo();
        let worktree =
            PathBuf::from(create_worktree(repo.to_str().unwrap(), "alpha", Some("main")).unwrap());
        let ctx = WorktreeContext {
            repo_root: repo.to_string_lossy().into_owned(),
            worktree_path: repo
                .join(".worktrees/missing")
                .to_string_lossy()
                .into_owned(),
            current_branch: "neozeus/alpha".into(),
            parent_branch: "main".into(),
            agent_name: "alpha".into(),
        };

        let error =
            remove_worktree_and_branch(&ctx).expect_err("dual failure should surface both causes");
        assert!(error.contains("failed to remove worktree"));
        assert!(error.contains("failed to delete branch"));
        assert!(worktree.exists());
    }

    #[test]
    fn active_sources_do_not_reference_legacy_worktree_prefix() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        collect_source_files(&manifest_dir.join("src"), &mut files);
        collect_source_files(&manifest_dir.join("scripts"), &mut files);
        files.sort();

        let offenders = files
            .into_iter()
            .filter_map(|path| {
                let text = std::fs::read_to_string(&path).ok()?;
                let text = if path.ends_with(Path::new("src/shared/worktree.rs")) {
                    text.split("#[cfg(test)]")
                        .next()
                        .unwrap_or(&text)
                        .to_owned()
                } else {
                    text
                };
                has_legacy_worktree_prefix(&text).then_some(path)
            })
            .collect::<Vec<_>>();

        assert!(
            offenders.is_empty(),
            "legacy `zeus/` worktree prefix leaked into active source files: {:?}",
            offenders
        );
    }
}
