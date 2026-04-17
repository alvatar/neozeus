use super::daemon_client::{OwnedTmuxCreator, SocketOwnedTmuxClient};
use neozeus::shared::{
    daemon_socket::daemon_socket_path_from_env_map, daemon_wire::OwnedTmuxSessionInfo,
    shell::shell_quote,
};
use std::{collections::HashMap, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
struct RunRequest {
    display_name: String,
    cwd: Option<String>,
    command: String,
    owner_agent_uid: String,
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let env = std::env::vars().collect::<HashMap<_, _>>();
    let request = parse_run_request(args, &env)?;
    let socket_path = socket_path_from_env(&env)?;
    let daemon = SocketOwnedTmuxClient::connect(Path::new(socket_path))?;
    let created = execute_run(&daemon, &request)?;
    println!("tmux attach -t {}", created.tmux_name);
    Ok(())
}

fn execute_run<D>(daemon: &D, request: &RunRequest) -> Result<OwnedTmuxSessionInfo, String>
where
    D: OwnedTmuxCreator,
{
    daemon.create_owned_tmux_session(
        &request.owner_agent_uid,
        &request.display_name,
        request.cwd.as_deref(),
        &request.command,
    )
}

fn socket_path_from_env(env: &HashMap<String, String>) -> Result<&str, String> {
    daemon_socket_path_from_env_map(env)
        .ok_or_else(|| "NEOZEUS_DAEMON_SOCKET_PATH or NEOZEUS_DAEMON_SOCKET is required".to_owned())
}

fn parse_run_request(args: &[String], env: &HashMap<String, String>) -> Result<RunRequest, String> {
    let owner_agent_uid = env
        .get("NEOZEUS_AGENT_UID")
        .cloned()
        .ok_or_else(|| "NEOZEUS_AGENT_UID is required".to_owned())?;
    if !matches!(args.first().map(String::as_str), Some("run")) {
        return Err(
            "usage: neozeus-tmux run --name <name> [--cwd <dir>] -- <command...>".to_owned(),
        );
    }

    let mut display_name = None;
    let mut cwd = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--name" => {
                index += 1;
                let value = args
                    .get(index)
                    .map(String::as_str)
                    .ok_or_else(|| "--name requires a value".to_owned())?;
                if value.trim().is_empty() {
                    return Err("--name must not be empty".to_owned());
                }
                display_name = Some(value.trim().to_owned());
                index += 1;
            }
            "--cwd" => {
                index += 1;
                let value = args
                    .get(index)
                    .map(String::as_str)
                    .ok_or_else(|| "--cwd requires a value".to_owned())?;
                if value.trim().is_empty() {
                    return Err("--cwd must not be empty".to_owned());
                }
                cwd = Some(value.trim().to_owned());
                index += 1;
            }
            "--" => {
                index += 1;
                break;
            }
            unknown => {
                return Err(format!(
                    "unknown argument `{unknown}`; usage: neozeus-tmux run --name <name> [--cwd <dir>] -- <command...>"
                ))
            }
        }
    }

    let display_name = display_name.ok_or_else(|| "--name is required".to_owned())?;
    let command_args = args
        .get(index..)
        .filter(|args| !args.is_empty())
        .ok_or_else(|| "command is required after `--`".to_owned())?;
    Ok(RunRequest {
        display_name,
        cwd,
        command: shell_join(command_args),
        owner_agent_uid,
    })
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeOwnedTmuxCreator {
        requests: Mutex<Vec<RunRequest>>,
    }

    impl OwnedTmuxCreator for FakeOwnedTmuxCreator {
        fn create_owned_tmux_session(
            &self,
            owner_agent_uid: &str,
            display_name: &str,
            cwd: Option<&str>,
            command: &str,
        ) -> Result<OwnedTmuxSessionInfo, String> {
            self.requests.lock().unwrap().push(RunRequest {
                display_name: display_name.to_owned(),
                cwd: cwd.map(str::to_owned),
                command: command.to_owned(),
                owner_agent_uid: owner_agent_uid.to_owned(),
            });
            Ok(OwnedTmuxSessionInfo {
                session_uid: "tmux-session-1".into(),
                owner_agent_uid: owner_agent_uid.to_owned(),
                tmux_name: "neozeus-tmux-1".into(),
                display_name: display_name.to_owned(),
                cwd: cwd.unwrap_or_default().to_owned(),
                attached: false,
                created_unix: 0,
            })
        }
    }

    fn env_with_agent() -> HashMap<String, String> {
        HashMap::from([
            ("NEOZEUS_AGENT_UID".to_owned(), "agent-uid-1".to_owned()),
            (
                "NEOZEUS_DAEMON_SOCKET_PATH".to_owned(),
                "/tmp/neozeus-test.sock".to_owned(),
            ),
        ])
    }

    #[test]
    fn parse_run_request_rejects_missing_agent_uid_env() {
        let error = parse_run_request(
            &[
                "run".into(),
                "--name".into(),
                "BUILD".into(),
                "--".into(),
                "cargo".into(),
            ],
            &HashMap::new(),
        )
        .expect_err("missing env should fail");
        assert_eq!(error, "NEOZEUS_AGENT_UID is required");
    }

    #[test]
    fn socket_path_from_env_requires_explicit_daemon_socket() {
        let error =
            socket_path_from_env(&HashMap::new()).expect_err("missing socket env should fail");
        assert_eq!(
            error,
            "NEOZEUS_DAEMON_SOCKET_PATH or NEOZEUS_DAEMON_SOCKET is required"
        );
    }

    #[test]
    fn socket_path_from_env_accepts_compatibility_socket_env() {
        let env = HashMap::from([(
            "NEOZEUS_DAEMON_SOCKET".to_owned(),
            "/tmp/legacy.sock".to_owned(),
        )]);
        assert_eq!(socket_path_from_env(&env).unwrap(), "/tmp/legacy.sock");
    }

    #[test]
    fn parse_run_request_accepts_name_cwd_and_command() {
        let request = parse_run_request(
            &[
                "run".into(),
                "--name".into(),
                "BUILD".into(),
                "--cwd".into(),
                "~/code".into(),
                "--".into(),
                "cargo".into(),
                "test".into(),
            ],
            &env_with_agent(),
        )
        .expect("request should parse");
        assert_eq!(request.display_name, "BUILD");
        assert_eq!(request.cwd.as_deref(), Some("~/code"));
        assert_eq!(request.command, "cargo test");
        assert_eq!(request.owner_agent_uid, "agent-uid-1");
    }

    #[test]
    fn parse_run_request_preserves_command_arguments_after_separator() {
        let request = parse_run_request(
            &[
                "run".into(),
                "--name".into(),
                "BUILD".into(),
                "--".into(),
                "printf".into(),
                "hello world".into(),
                "$HOME".into(),
                "a'b".into(),
            ],
            &env_with_agent(),
        )
        .expect("request should parse");
        assert_eq!(request.command, "printf 'hello world' '$HOME' 'a'\\''b'");
    }

    #[test]
    fn execute_run_sends_expected_owned_tmux_creation_request() {
        let creator = FakeOwnedTmuxCreator::default();
        let request = parse_run_request(
            &[
                "run".into(),
                "--name".into(),
                "BUILD".into(),
                "--cwd".into(),
                "/tmp/work".into(),
                "--".into(),
                "cargo".into(),
                "test".into(),
            ],
            &env_with_agent(),
        )
        .expect("request should parse");

        let created = execute_run(&creator, &request).expect("run should succeed");
        assert_eq!(created.tmux_name, "neozeus-tmux-1");

        let requests = creator.requests.lock().unwrap();
        assert_eq!(requests.as_slice(), &[request]);
    }
}
