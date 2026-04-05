use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn prepare_git_command(cwd: &str) -> Command {
    let mut command = Command::new("git");
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

const WORKTREE_DIR: &str = ".worktrees";
const WORKTREE_BRANCH_PREFIX: &str = "zeus/";

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

/// Resolves the canonical/common repository root for a checkout path.
pub fn get_worktree_repo_root(cwd: &str) -> Result<String, String> {
    let common_dir = run_git_capture(cwd, &["rev-parse", "--git-common-dir"], 5)?;
    let common_dir = if Path::new(&common_dir).is_absolute() {
        PathBuf::from(common_dir)
    } else {
        Path::new(cwd).join(common_dir)
    };
    let common_dir = common_dir
        .canonicalize()
        .unwrap_or(common_dir)
        .to_string_lossy()
        .into_owned();

    if Path::new(&common_dir)
        .file_name()
        .and_then(|value| value.to_str())
        == Some(".git")
    {
        return Ok(
            Path::new(&common_dir)
                .parent()
                .ok_or_else(|| format!("failed to resolve repo root from `{common_dir}`"))?
                .to_string_lossy()
                .into_owned(),
        );
    }

    get_repo_root(cwd)
}

/// Returns the checkout root for one git directory.
pub fn get_repo_root(cwd: &str) -> Result<String, String> {
    run_git_capture(cwd, &["rev-parse", "--show-toplevel"], 5)
}

/// Returns the current branch name for one git checkout.
pub fn get_current_branch(cwd: &str) -> Result<String, String> {
    run_git_capture(cwd, &["rev-parse", "--abbrev-ref", "HEAD"], 5)
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

fn run_git_capture(cwd: &str, args: &[&str], timeout_secs: u64) -> Result<String, String> {
    let output = prepare_git_command(cwd)
        .args(args)
        .output()
        .map_err(|error| format!("failed to execute git {:?} in {}: {error}", args, cwd))?;
    let _ = timeout_secs;
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
        create_worktree, get_current_branch, get_repo_root, get_worktree_repo_root,
        worktree_base_dir, worktree_branch, worktree_path,
    };
    use std::{path::PathBuf, process::Command};

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = PathBuf::from("/tmp").join(format!(
            "neozeus-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&path).expect("temp dir should create");
        path
    }

    fn run(repo_root: &PathBuf, args: &[&str]) {
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

    fn init_git_repo() -> PathBuf {
        let repo = temp_dir("worktree-repo");
        run(&repo, &["git", "init"]);
        run(&repo, &["git", "config", "user.email", "neozeus@example.test"]);
        run(&repo, &["git", "config", "user.name", "NeoZeus Test"]);
        std::fs::write(repo.join("README.md"), "seed\n").unwrap();
        run(&repo, &["git", "add", "README.md"]);
        run(&repo, &["git", "commit", "-m", "initial"]);
        run(&repo, &["git", "branch", "-M", "main"]);
        repo
    }

    #[test]
    fn worktree_path_and_branch_follow_zeus_layout() {
        let repo_root = "/tmp/project";
        assert_eq!(worktree_base_dir(repo_root), PathBuf::from("/tmp/project/.worktrees"));
        assert_eq!(
            worktree_path(repo_root, "alpha"),
            PathBuf::from("/tmp/project/.worktrees/alpha")
        );
        assert_eq!(worktree_branch("alpha"), "zeus/alpha");
    }

    #[test]
    fn get_worktree_repo_root_resolves_main_checkout_root() {
        let repo = init_git_repo();
        assert_eq!(get_repo_root(repo.to_str().unwrap()).unwrap(), repo.to_string_lossy());
        assert_eq!(
            get_worktree_repo_root(repo.to_str().unwrap()).unwrap(),
            repo.to_string_lossy()
        );
        assert_eq!(get_current_branch(repo.to_str().unwrap()).unwrap(), "main");
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
    }

    #[test]
    fn create_worktree_creates_real_checkout_branch_and_gitignore_entry() {
        let repo = init_git_repo();
        let worktree = create_worktree(repo.to_str().unwrap(), "beta", Some("main")).unwrap();
        let worktree = PathBuf::from(worktree);

        assert!(worktree.is_dir());
        assert_eq!(
            get_current_branch(worktree.to_str().unwrap()).unwrap(),
            "zeus/beta"
        );
        let gitignore = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert_eq!(gitignore.lines().filter(|line| line.trim() == "/.worktrees/").count(), 1);
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
        let error = get_worktree_repo_root(dir.to_str().unwrap())
            .expect_err("non-git cwd should fail");
        assert!(error.contains("git"));
    }
}
