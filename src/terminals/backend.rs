#[cfg(test)]
pub(crate) use crate::terminals::ansi_surface::{resolve_alacritty_color, xterm_indexed_rgb};
pub(crate) use crate::terminals::damage::compute_terminal_damage;

// Implements send command payload bytes.
pub(crate) fn send_command_payload_bytes(command: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(command.len() + 1);
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if matches!(chars.peek(), Some('\n')) {
                    let _ = chars.next();
                }
                bytes.push(b'\r');
            }
            '\n' => bytes.push(b'\r'),
            _ => {
                let mut encoded = [0_u8; 4];
                bytes.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            }
        }
    }
    bytes.push(b'\r');
    bytes
}
