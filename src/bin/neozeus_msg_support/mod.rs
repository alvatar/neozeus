#[path = "../../shared/app_state_file.rs"]
mod app_state_file;
mod daemon_client;
#[path = "../../shared/daemon_socket.rs"]
mod daemon_socket;
#[cfg(test)]
#[path = "../../shared/send_command.rs"]
mod send_command;

use self::{
    app_state_file::PersistedAppState,
    daemon_client::{DaemonMessenger, DaemonSessionInfo, SocketDaemonMessenger, TerminalCommand},
};
use std::{fs, path::Path};

const USAGE: &str =
    "usage: neozeus-msg send (--to-agent <agent-name> | --to-session <session-name>) <message>";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Command {
    Send(SendRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SendRequest {
    target: SendTarget,
    message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SendTarget {
    Agent(String),
    Session(String),
}

/// Runs the standalone `neozeus-msg` entrypoint.
pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let command = parse_args(args)?;
    match command {
        Command::Send(request) => {
            let daemon = SocketDaemonMessenger::connect_or_start_default()?;
            let persisted = match request.target {
                SendTarget::Agent(_) => Some(load_cli_persisted_app_state()?),
                SendTarget::Session(_) => None,
            };
            let session_name = dispatch_send(&daemon, &request, persisted.as_ref())?;
            println!("sent to {session_name}");
            Ok(())
        }
    }
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    let Some((command, rest)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };
    if command != "send" {
        return Err(USAGE.to_owned());
    }

    let mut to_agent = None;
    let mut to_session = None;
    let mut positionals = Vec::new();
    let mut index = 0usize;
    while index < rest.len() {
        match rest[index].as_str() {
            "--to-agent" => {
                index += 1;
                let Some(value) = rest.get(index) else {
                    return Err("--to-agent requires an agent name".to_owned());
                };
                to_agent = Some(value.clone());
            }
            "--to-session" => {
                index += 1;
                let Some(value) = rest.get(index) else {
                    return Err("--to-session requires a session name".to_owned());
                };
                to_session = Some(value.clone());
            }
            "--" => {
                positionals.extend(rest[index + 1..].iter().cloned());
                break;
            }
            value if value.starts_with("--") => return Err(format!("unknown flag `{value}`")),
            value => positionals.push(value.to_owned()),
        }
        index += 1;
    }

    let target = match (to_agent, to_session) {
        (Some(_), Some(_)) | (None, None) => {
            return Err("exactly one of --to-agent or --to-session must be provided".to_owned())
        }
        (Some(agent_name), None) => SendTarget::Agent(agent_name),
        (None, Some(session_name)) => SendTarget::Session(session_name),
    };

    if positionals.is_empty() {
        return Err("missing message argument".to_owned());
    }
    if positionals.len() > 1 {
        return Err("message must be passed as one quoted argument".to_owned());
    }
    let message = positionals.into_iter().next().unwrap();
    if message.trim().is_empty() {
        return Err("message must not be empty".to_owned());
    }

    Ok(Command::Send(SendRequest { target, message }))
}

fn load_cli_persisted_app_state() -> Result<PersistedAppState, String> {
    let Some(path) = app_state_file::resolve_app_state_path() else {
        return Err("failed to resolve app state path".to_owned());
    };
    load_existing_persisted_app_state(&path)
}

fn load_existing_persisted_app_state(path: &Path) -> Result<PersistedAppState, String> {
    if !path.exists() {
        return Err(format!("app state file not found: {}", path.display()));
    }
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read app state {}: {error}", path.display()))?;
    Ok(app_state_file::parse_persisted_app_state(&text))
}

fn resolve_session_from_agent_label(
    persisted: &PersistedAppState,
    label: &str,
) -> Result<String, String> {
    let matches = persisted
        .agents
        .iter()
        .filter(|record| record.label.as_deref() == Some(label))
        .map(|record| record.session_name.clone())
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(format!("unknown agent `{label}`")),
        1 => Ok(matches[0].clone()),
        _ => Err(format!(
            "agent `{label}` resolves to multiple sessions in app state"
        )),
    }
}

fn verify_live_session_exists(
    sessions: &[DaemonSessionInfo],
    session_name: &str,
) -> Result<(), String> {
    sessions
        .iter()
        .any(|session| session.session_id == session_name)
        .then_some(())
        .ok_or_else(|| format!("daemon session `{session_name}` not found"))
}

fn dispatch_send<D: DaemonMessenger>(
    daemon: &D,
    request: &SendRequest,
    persisted: Option<&PersistedAppState>,
) -> Result<String, String> {
    let sessions = daemon.list_sessions()?;
    let session_name = match &request.target {
        SendTarget::Agent(label) => {
            let persisted = persisted
                .ok_or_else(|| "agent targeting requires a loaded app-state mapping".to_owned())?;
            resolve_session_from_agent_label(persisted, label)?
        }
        SendTarget::Session(session_name) => session_name.clone(),
    };
    verify_live_session_exists(&sessions, &session_name)?;
    daemon.send_command(
        &session_name,
        TerminalCommand::SendCommand(request.message.clone()),
    )?;
    Ok(session_name)
}

#[cfg(test)]
mod tests {
    use super::{
        app_state_file::{PersistedAgentState, PersistedAppState},
        daemon_client::{
            DaemonMessenger, DaemonSessionInfo, TerminalCommand, TerminalRuntimeState,
        },
        dispatch_send, load_existing_persisted_app_state, parse_args,
        resolve_session_from_agent_label,
        send_command::send_command_payload_bytes,
        Command, SendRequest, SendTarget,
    };
    use std::{
        fs,
        path::PathBuf,
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[derive(Default)]
    struct FakeDaemon {
        sessions: Vec<DaemonSessionInfo>,
        sent_commands: Mutex<Vec<(String, TerminalCommand)>>,
    }

    impl DaemonMessenger for FakeDaemon {
        fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
            Ok(self.sessions.clone())
        }

        fn send_command(&self, session_name: &str, command: TerminalCommand) -> Result<(), String> {
            self.sent_commands
                .lock()
                .unwrap()
                .push((session_name.to_owned(), command));
            Ok(())
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("neozeus-msg-{name}-{unique}"))
    }

    fn session_info(session_id: &str) -> DaemonSessionInfo {
        DaemonSessionInfo {
            session_id: session_id.to_owned(),
            runtime: TerminalRuntimeState::running("fake daemon"),
            revision: 0,
            created_order: 0,
        }
    }

    fn persisted_with_agents(records: &[(&str, Option<&str>)]) -> PersistedAppState {
        PersistedAppState {
            agents: records
                .iter()
                .enumerate()
                .map(|(index, (session_name, label))| PersistedAgentState {
                    session_name: (*session_name).to_owned(),
                    label: label.map(str::to_owned),
                    order_index: index as u64,
                    last_focused: false,
                })
                .collect(),
        }
    }

    #[test]
    fn parse_args_rejects_both_target_modes() {
        let error = parse_args(&[
            "send".into(),
            "--to-agent".into(),
            "alpha".into(),
            "--to-session".into(),
            "session-1".into(),
            "hello".into(),
        ])
        .expect_err("both target modes should fail");
        assert!(error.contains("exactly one of --to-agent or --to-session"));
    }

    #[test]
    fn parse_args_rejects_missing_target_mode() {
        let error =
            parse_args(&["send".into(), "hello".into()]).expect_err("missing target should fail");
        assert!(error.contains("exactly one of --to-agent or --to-session"));
    }

    #[test]
    fn parse_args_rejects_missing_message_argument() {
        let error = parse_args(&["send".into(), "--to-agent".into(), "alpha".into()])
            .expect_err("missing message should fail");
        assert_eq!(error, "missing message argument");
    }

    #[test]
    fn parse_args_rejects_empty_message_argument() {
        let error = parse_args(&[
            "send".into(),
            "--to-session".into(),
            "session-1".into(),
            "   ".into(),
        ])
        .expect_err("empty message should fail");
        assert_eq!(error, "message must not be empty");
    }

    #[test]
    fn parse_args_accepts_agent_target_and_message() {
        let command = parse_args(&[
            "send".into(),
            "--to-agent".into(),
            "alpha".into(),
            "hello world".into(),
        ])
        .expect("valid send command should parse");
        assert_eq!(
            command,
            Command::Send(SendRequest {
                target: SendTarget::Agent("alpha".into()),
                message: "hello world".into(),
            })
        );
    }

    #[test]
    fn load_existing_persisted_app_state_fails_when_file_is_missing() {
        let path = temp_path("missing-app-state");
        let error = load_existing_persisted_app_state(&path)
            .expect_err("missing app-state file should fail");
        assert!(error.contains("app state file not found"));
    }

    #[test]
    fn resolve_session_from_agent_label_uses_persisted_mapping() {
        let persisted =
            persisted_with_agents(&[("session-a", Some("alpha")), ("session-b", Some("beta"))]);
        assert_eq!(
            resolve_session_from_agent_label(&persisted, "beta").unwrap(),
            "session-b"
        );
    }

    #[test]
    fn resolve_session_from_agent_label_rejects_unknown_labels() {
        let persisted = persisted_with_agents(&[("session-a", Some("alpha"))]);
        let error = resolve_session_from_agent_label(&persisted, "missing")
            .expect_err("unknown label should fail");
        assert_eq!(error, "unknown agent `missing`");
    }

    #[test]
    fn resolve_session_from_agent_label_rejects_duplicate_labels() {
        let persisted =
            persisted_with_agents(&[("session-a", Some("alpha")), ("session-b", Some("alpha"))]);
        let error = resolve_session_from_agent_label(&persisted, "alpha")
            .expect_err("duplicate labels should fail");
        assert!(error.contains("resolves to multiple sessions"));
    }

    #[test]
    fn dispatch_send_to_session_sends_to_requested_session() {
        let daemon = FakeDaemon {
            sessions: vec![session_info("session-1")],
            ..Default::default()
        };
        let request = SendRequest {
            target: SendTarget::Session("session-1".into()),
            message: "echo hi".into(),
        };

        let sent_to = dispatch_send(&daemon, &request, None).expect("send should succeed");

        assert_eq!(sent_to, "session-1");
        assert_eq!(
            daemon.sent_commands.lock().unwrap().as_slice(),
            &[(
                "session-1".to_owned(),
                TerminalCommand::SendCommand("echo hi".into())
            )]
        );
    }

    #[test]
    fn dispatch_send_to_agent_resolves_label_then_sends_to_session() {
        let daemon = FakeDaemon {
            sessions: vec![session_info("session-b")],
            ..Default::default()
        };
        let persisted = persisted_with_agents(&[("session-b", Some("beta"))]);
        let request = SendRequest {
            target: SendTarget::Agent("beta".into()),
            message: "status".into(),
        };

        let sent_to =
            dispatch_send(&daemon, &request, Some(&persisted)).expect("send should succeed");

        assert_eq!(sent_to, "session-b");
        assert_eq!(
            daemon.sent_commands.lock().unwrap().as_slice(),
            &[(
                "session-b".to_owned(),
                TerminalCommand::SendCommand("status".into())
            )]
        );
    }

    #[test]
    fn dispatch_send_rejects_stale_agent_mapping_when_session_is_not_live() {
        let daemon = FakeDaemon::default();
        let persisted = persisted_with_agents(&[("session-b", Some("beta"))]);
        let request = SendRequest {
            target: SendTarget::Agent("beta".into()),
            message: "status".into(),
        };

        let error = dispatch_send(&daemon, &request, Some(&persisted))
            .expect_err("stale daemon mapping should fail");

        assert_eq!(error, "daemon session `session-b` not found");
    }

    #[test]
    fn dispatch_send_preserves_multiline_enter_semantics() {
        let daemon = FakeDaemon {
            sessions: vec![session_info("session-1")],
            ..Default::default()
        };
        let request = SendRequest {
            target: SendTarget::Session("session-1".into()),
            message: "echo hi\npwd".into(),
        };

        dispatch_send(&daemon, &request, None).expect("send should succeed");

        let sent = daemon.sent_commands.lock().unwrap();
        let (_, TerminalCommand::SendCommand(payload)) = &sent[0];
        assert_eq!(payload, "echo hi\npwd");
        assert_eq!(send_command_payload_bytes(payload), b"echo hi\rpwd\r");
    }

    #[test]
    fn load_existing_persisted_app_state_reads_current_app_state_format() {
        let path = temp_path("load-current-app-state");
        fs::write(
            &path,
            "neozeus state version 1\n[agent]\nsession_name=\"session-a\"\nlabel=\"alpha\"\norder_index=0\nfocused=1\n[/agent]\n",
        )
        .unwrap();

        let persisted =
            load_existing_persisted_app_state(&path).expect("app-state load should succeed");

        assert_eq!(persisted.agents.len(), 1);
        assert_eq!(persisted.agents[0].session_name, "session-a");
        assert_eq!(persisted.agents[0].label.as_deref(), Some("alpha"));
    }
}
