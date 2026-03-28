#[cfg(test)]
pub(crate) use super::ansi_surface::{resolve_alacritty_color, xterm_indexed_rgb};
pub(crate) use super::damage::compute_terminal_damage;

/// Converts a command string into the byte stream that should be written to the PTY.
///
/// The main subtlety is newline normalization: both bare `\n` and CRLF are collapsed to carriage
/// returns because the terminal command path wants "press Enter" semantics for each logical line. A
/// trailing carriage return is always appended so even a single-line command is submitted.
pub(crate) fn send_command_payload_bytes(command: &str) -> Vec<u8> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
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
