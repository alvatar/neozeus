use neozeus::shared::worktree::{
    ensure_clean_worktree, merge_parent_back_into_worktree, merge_worktree_into_parent,
    remove_worktree_and_branch, resolve_worktree_context, WorktreeContext,
};

const USAGE: &str =
    "usage: neozeus-worktree <merge-continue|merge-finalize|discard> [--parent-branch <branch>]";

#[derive(Clone, Debug, PartialEq, Eq)]
enum CommandKind {
    MergeContinue,
    MergeFinalize,
    Discard,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Command {
    kind: CommandKind,
    parent_branch_override: Option<String>,
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let command = parse_args(args)?;
    execute_with_ops(&SocketWorktreeLifecycleOps, &command, |line| println!("{line}"))
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    let Some((first, rest)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };
    let kind = match first.as_str() {
        "merge-continue" => CommandKind::MergeContinue,
        "merge-finalize" => CommandKind::MergeFinalize,
        "discard" => CommandKind::Discard,
        _ => return Err(USAGE.to_owned()),
    };

    let mut parent_branch_override = None;
    let mut index = 0usize;
    while index < rest.len() {
        match rest[index].as_str() {
            "--parent-branch" => {
                index += 1;
                let Some(value) = rest.get(index) else {
                    return Err("--parent-branch requires a value".to_owned());
                };
                let value = value.trim();
                if value.is_empty() {
                    return Err("--parent-branch must not be empty".to_owned());
                }
                if parent_branch_override.replace(value.to_owned()).is_some() {
                    return Err("--parent-branch may only be provided once".to_owned());
                }
            }
            value => return Err(format!("unknown argument `{value}`; {USAGE}")),
        }
        index += 1;
    }

    Ok(Command {
        kind,
        parent_branch_override,
    })
}

trait WorktreeLifecycleOps {
    fn resolve_worktree_context(
        &self,
        cwd: &str,
        parent_branch_override: Option<&str>,
    ) -> Result<WorktreeContext, String>;
    fn ensure_clean_worktree(&self, cwd: &str) -> Result<(), String>;
    fn merge_worktree_into_parent(&self, ctx: &WorktreeContext) -> Result<String, String>;
    fn merge_parent_back_into_worktree(&self, ctx: &WorktreeContext) -> Result<String, String>;
    fn remove_worktree_and_branch(&self, ctx: &WorktreeContext) -> Result<(), String>;
}

struct SocketWorktreeLifecycleOps;

impl WorktreeLifecycleOps for SocketWorktreeLifecycleOps {
    fn resolve_worktree_context(
        &self,
        cwd: &str,
        parent_branch_override: Option<&str>,
    ) -> Result<WorktreeContext, String> {
        resolve_worktree_context(cwd, parent_branch_override)
    }

    fn ensure_clean_worktree(&self, cwd: &str) -> Result<(), String> {
        ensure_clean_worktree(cwd)
    }

    fn merge_worktree_into_parent(&self, ctx: &WorktreeContext) -> Result<String, String> {
        merge_worktree_into_parent(ctx)
    }

    fn merge_parent_back_into_worktree(&self, ctx: &WorktreeContext) -> Result<String, String> {
        merge_parent_back_into_worktree(ctx)
    }

    fn remove_worktree_and_branch(&self, ctx: &WorktreeContext) -> Result<(), String> {
        remove_worktree_and_branch(ctx)
    }
}

fn execute_with_ops<O, F>(ops: &O, command: &Command, mut emit: F) -> Result<(), String>
where
    O: WorktreeLifecycleOps,
    F: FnMut(String),
{
    let cwd = std::env::current_dir()
        .map_err(|error| format!("failed to resolve current directory: {error}"))?;
    let cwd = cwd.to_string_lossy().into_owned();
    let context = ops.resolve_worktree_context(&cwd, command.parent_branch_override.as_deref())?;
    match command.kind {
        CommandKind::MergeContinue => {
            ops.ensure_clean_worktree(&context.worktree_path)?;
            ops.merge_worktree_into_parent(&context)?;
            emit(format!(
                "merged {} into {}",
                context.current_branch, context.parent_branch
            ));
            ops.merge_parent_back_into_worktree(&context)?;
            emit(format!(
                "synced {} back into {}",
                context.parent_branch, context.current_branch
            ));
        }
        CommandKind::MergeFinalize => {
            ops.ensure_clean_worktree(&context.worktree_path)?;
            ops.merge_worktree_into_parent(&context)?;
            emit(format!(
                "merged {} into {}",
                context.current_branch, context.parent_branch
            ));
            ops.remove_worktree_and_branch(&context)?;
            emit(format!(
                "removed {} and deleted {}",
                context.worktree_path, context.current_branch
            ));
        }
        CommandKind::Discard => {
            ops.remove_worktree_and_branch(&context)?;
            emit(format!(
                "discarded {} and removed {}",
                context.current_branch, context.worktree_path
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone)]
    enum Outcome<T> {
        Ok(T),
        Err(String),
    }

    struct FakeOps {
        calls: Mutex<Vec<String>>,
        context: Outcome<WorktreeContext>,
        clean: Outcome<()>,
        merge_parent: Outcome<String>,
        merge_back: Outcome<String>,
        remove: Outcome<()>,
    }

    impl Default for FakeOps {
        fn default() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                context: Outcome::Ok(fake_context()),
                clean: Outcome::Ok(()),
                merge_parent: Outcome::Ok("merged".into()),
                merge_back: Outcome::Ok("merged-back".into()),
                remove: Outcome::Ok(()),
            }
        }
    }

    impl WorktreeLifecycleOps for FakeOps {
        fn resolve_worktree_context(
            &self,
            cwd: &str,
            parent_branch_override: Option<&str>,
        ) -> Result<WorktreeContext, String> {
            self.calls.lock().unwrap().push(format!(
                "resolve:{cwd}:{}",
                parent_branch_override.unwrap_or("")
            ));
            match &self.context {
                Outcome::Ok(context) => Ok(context.clone()),
                Outcome::Err(error) => Err(error.clone()),
            }
        }

        fn ensure_clean_worktree(&self, cwd: &str) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("clean:{cwd}"));
            match &self.clean {
                Outcome::Ok(()) => Ok(()),
                Outcome::Err(error) => Err(error.clone()),
            }
        }

        fn merge_worktree_into_parent(&self, _ctx: &WorktreeContext) -> Result<String, String> {
            self.calls.lock().unwrap().push("merge-parent".into());
            match &self.merge_parent {
                Outcome::Ok(output) => Ok(output.clone()),
                Outcome::Err(error) => Err(error.clone()),
            }
        }

        fn merge_parent_back_into_worktree(&self, _ctx: &WorktreeContext) -> Result<String, String> {
            self.calls.lock().unwrap().push("merge-back".into());
            match &self.merge_back {
                Outcome::Ok(output) => Ok(output.clone()),
                Outcome::Err(error) => Err(error.clone()),
            }
        }

        fn remove_worktree_and_branch(&self, _ctx: &WorktreeContext) -> Result<(), String> {
            self.calls.lock().unwrap().push("remove".into());
            match &self.remove {
                Outcome::Ok(()) => Ok(()),
                Outcome::Err(error) => Err(error.clone()),
            }
        }
    }

    fn fake_context() -> WorktreeContext {
        WorktreeContext {
            repo_root: "/repo".into(),
            worktree_path: "/repo/.worktrees/alpha".into(),
            current_branch: "neozeus/alpha".into(),
            parent_branch: "main".into(),
            agent_name: "alpha".into(),
        }
    }

    #[test]
    fn parse_args_accepts_merge_continue() {
        let command = parse_args(&["merge-continue".into()]).unwrap();
        assert_eq!(
            command,
            Command {
                kind: CommandKind::MergeContinue,
                parent_branch_override: None,
            }
        );
    }

    #[test]
    fn parse_args_accepts_merge_finalize_with_parent_override() {
        let command = parse_args(&[
            "merge-finalize".into(),
            "--parent-branch".into(),
            "release".into(),
        ])
        .unwrap();
        assert_eq!(
            command,
            Command {
                kind: CommandKind::MergeFinalize,
                parent_branch_override: Some("release".into()),
            }
        );
    }

    #[test]
    fn parse_args_accepts_discard() {
        let command = parse_args(&["discard".into()]).unwrap();
        assert_eq!(
            command,
            Command {
                kind: CommandKind::Discard,
                parent_branch_override: None,
            }
        );
    }

    #[test]
    fn parse_args_rejects_unknown_subcommand() {
        let error = parse_args(&["merge".into()]).expect_err("unknown subcommand should fail");
        assert_eq!(error, USAGE);
    }

    #[test]
    fn parse_args_rejects_extra_arguments() {
        let error = parse_args(&["discard".into(), "extra".into()])
            .expect_err("extra arg should fail");
        assert!(error.contains("unknown argument `extra`"));
    }

    #[test]
    fn parse_args_rejects_missing_parent_branch_value() {
        let error = parse_args(&["merge-continue".into(), "--parent-branch".into()])
            .expect_err("missing override should fail");
        assert_eq!(error, "--parent-branch requires a value");
    }

    #[test]
    fn parse_args_rejects_empty_parent_branch_value() {
        let error = parse_args(&[
            "merge-continue".into(),
            "--parent-branch".into(),
            "   ".into(),
        ])
        .expect_err("empty override should fail");
        assert_eq!(error, "--parent-branch must not be empty");
    }

    #[test]
    fn parse_args_rejects_duplicate_parent_branch_flag() {
        let error = parse_args(&[
            "merge-continue".into(),
            "--parent-branch".into(),
            "release".into(),
            "--parent-branch".into(),
            "main".into(),
        ])
        .expect_err("duplicate override should fail");
        assert_eq!(error, "--parent-branch may only be provided once");
    }

    #[test]
    fn merge_continue_calls_ops_in_order() {
        let ops = FakeOps::default();
        let command = Command {
            kind: CommandKind::MergeContinue,
            parent_branch_override: Some("release".into()),
        };
        let mut lines = Vec::new();

        execute_with_ops(&ops, &command, |line| lines.push(line)).unwrap();

        assert_eq!(
            ops.calls.lock().unwrap().as_slice(),
            &[
                format!("resolve:{}:release", std::env::current_dir().unwrap().to_string_lossy()),
                "clean:/repo/.worktrees/alpha".into(),
                "merge-parent".into(),
                "merge-back".into(),
            ]
        );
        assert_eq!(
            lines,
            vec![
                "merged neozeus/alpha into main".to_owned(),
                "synced main back into neozeus/alpha".to_owned(),
            ]
        );
    }

    #[test]
    fn merge_finalize_calls_ops_in_order() {
        let ops = FakeOps::default();
        let command = Command {
            kind: CommandKind::MergeFinalize,
            parent_branch_override: None,
        };
        let mut lines = Vec::new();

        execute_with_ops(&ops, &command, |line| lines.push(line)).unwrap();

        assert_eq!(
            ops.calls.lock().unwrap().as_slice(),
            &[
                format!("resolve:{}:", std::env::current_dir().unwrap().to_string_lossy()),
                "clean:/repo/.worktrees/alpha".into(),
                "merge-parent".into(),
                "remove".into(),
            ]
        );
        assert_eq!(
            lines,
            vec![
                "merged neozeus/alpha into main".to_owned(),
                "removed /repo/.worktrees/alpha and deleted neozeus/alpha".to_owned(),
            ]
        );
    }

    #[test]
    fn discard_skips_merge_steps() {
        let ops = FakeOps::default();
        let command = Command {
            kind: CommandKind::Discard,
            parent_branch_override: None,
        };
        let mut lines = Vec::new();

        execute_with_ops(&ops, &command, |line| lines.push(line)).unwrap();

        assert_eq!(
            ops.calls.lock().unwrap().as_slice(),
            &[
                format!("resolve:{}:", std::env::current_dir().unwrap().to_string_lossy()),
                "remove".into(),
            ]
        );
        assert_eq!(
            lines,
            vec!["discarded neozeus/alpha and removed /repo/.worktrees/alpha".to_owned()]
        );
    }

    #[test]
    fn safety_failures_stop_later_steps() {
        let ops = FakeOps {
            clean: Outcome::Err("dirty checkout".into()),
            ..Default::default()
        };
        let command = Command {
            kind: CommandKind::MergeFinalize,
            parent_branch_override: None,
        };

        let error = execute_with_ops(&ops, &command, |_| {}).expect_err("clean check should fail");

        assert_eq!(error, "dirty checkout");
        assert_eq!(
            ops.calls.lock().unwrap().as_slice(),
            &[
                format!("resolve:{}:", std::env::current_dir().unwrap().to_string_lossy()),
                "clean:/repo/.worktrees/alpha".into(),
            ]
        );
    }

    #[test]
    fn merge_back_errors_preserve_detail() {
        let ops = FakeOps {
            merge_back: Outcome::Err("failed to merge main into neozeus/alpha; conflicted files: README.md".into()),
            ..Default::default()
        };
        let command = Command {
            kind: CommandKind::MergeContinue,
            parent_branch_override: None,
        };

        let error = execute_with_ops(&ops, &command, |_| {}).expect_err("merge-back should fail");

        assert!(error.contains("README.md"));
        assert!(error.contains("neozeus/alpha"));
    }

    #[test]
    fn resolve_failures_stop_all_destructive_steps() {
        let ops = FakeOps {
            context: Outcome::Err("not in linked worktree".into()),
            ..Default::default()
        };
        let command = Command {
            kind: CommandKind::Discard,
            parent_branch_override: None,
        };

        let error = execute_with_ops(&ops, &command, |_| {}).expect_err("resolve should fail");

        assert_eq!(error, "not in linked worktree");
        assert_eq!(
            ops.calls.lock().unwrap().as_slice(),
            &[format!("resolve:{}:", std::env::current_dir().unwrap().to_string_lossy())]
        );
    }
}
