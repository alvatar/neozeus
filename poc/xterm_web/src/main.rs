use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Deserialize;
use std::{
    env,
    ffi::OsString,
    io::{Read, Write},
    net::SocketAddr,
    sync::Arc,
    thread,
};
use tokio::{net::TcpListener, sync::mpsc};

const DEFAULT_ROWS: u16 = 40;
const DEFAULT_COLS: u16 = 120;
const INDEX_HTML: &str = include_str!("../static/index.html");

#[derive(Clone)]
struct AppState {
    shell: Arc<OsString>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
}

struct ShellSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[tokio::main]
async fn main() {
    let app_state = AppState {
        shell: Arc::new(shell_path()),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_handler))
        .with_state(app_state);

    let address: SocketAddr = "127.0.0.1:3001"
        .parse()
        .expect("hardcoded socket address must parse");
    let listener = TcpListener::bind(address)
        .await
        .expect("failed to bind xterm PoC server");

    println!("neozeus :: xterm.js PoC listening on http://{address}");

    axum::serve(listener, app)
        .await
        .expect("xterm PoC server crashed");
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let mut session = match spawn_shell_session(&state.shell, DEFAULT_COLS, DEFAULT_ROWS) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("failed to start shell session: {error}");
            return;
        }
    };

    let mut reader = match session.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("failed to clone PTY reader: {error}");
            let _ = session.child.kill();
            return;
        }
    };

    let (mut sender, mut receiver) = socket.split();
    let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(64);

    let reader_thread = thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if output_tx.blocking_send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    eprintln!("PTY read error: {error}");
                    break;
                }
            }
        }
    });

    let sender_task = tokio::spawn(async move {
        let _ = sender
            .send(Message::Text(
                r#"{"type":"status","message":"connected"}"#.into(),
            ))
            .await;

        while let Some(chunk) = output_rx.recv().await {
            if sender.send(Message::Binary(chunk.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(message_result) = receiver.next().await {
        let message = match message_result {
            Ok(message) => message,
            Err(error) => {
                eprintln!("websocket receive error: {error}");
                break;
            }
        };

        match message {
            Message::Text(text) => {
                if let Some(client_message) = parse_client_message(&text) {
                    match client_message {
                        ClientMessage::Input { data } => {
                            if session.writer.write_all(data.as_bytes()).is_err() {
                                break;
                            }
                            if session.writer.flush().is_err() {
                                break;
                            }
                        }
                        ClientMessage::Resize { cols, rows } => {
                            let size = PtySize {
                                rows: rows.max(2),
                                cols: cols.max(2),
                                pixel_width: 0,
                                pixel_height: 0,
                            };
                            if session.master.resize(size).is_err() {
                                break;
                            }
                        }
                    }
                }
            }
            Message::Binary(_) => {}
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }

    sender_task.abort();
    let _ = session.child.kill();
    let _ = reader_thread.join();
}

fn spawn_shell_session(shell: &OsString, cols: u16, rows: u16) -> Result<ShellSession, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("openpty failed: {error}"))?;

    let mut command = CommandBuilder::new(shell);
    command.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("spawn_command failed: {error}"))?;

    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("take_writer failed: {error}"))?;

    Ok(ShellSession {
        master: pair.master,
        writer,
        child,
    })
}

fn shell_path() -> OsString {
    match env::var_os("SHELL") {
        Some(shell) => shell,
        None => OsString::from("bash"),
    }
}

fn parse_client_message(text: &str) -> Option<ClientMessage> {
    serde_json::from_str(text).ok()
}

#[cfg(test)]
mod tests {
    use super::{parse_client_message, ClientMessage};

    #[test]
    fn parses_input_messages() {
        let message = parse_client_message(r#"{"type":"input","data":"ls\r"}"#);
        assert_eq!(
            message,
            Some(ClientMessage::Input {
                data: "ls\r".into()
            })
        );
    }

    #[test]
    fn parses_resize_messages() {
        let message = parse_client_message(r#"{"type":"resize","cols":132,"rows":43}"#);
        assert_eq!(
            message,
            Some(ClientMessage::Resize {
                cols: 132,
                rows: 43,
            })
        );
    }

    #[test]
    fn rejects_invalid_messages() {
        assert_eq!(parse_client_message("not json"), None);
    }
}
