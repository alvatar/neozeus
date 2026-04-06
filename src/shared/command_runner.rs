use std::{
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct CommandOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command, kill_process_group: bool) {
    if !kill_process_group {
        return;
    }
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc_setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command, _kill_process_group: bool) {}

#[cfg(unix)]
fn kill_child_or_group(child: &mut Child, kill_process_group: bool) {
    if kill_process_group {
        unsafe {
            let _ = libc_kill(-(child.id() as i32), 9);
        }
    } else {
        let _ = child.kill();
    }
}

#[cfg(not(unix))]
fn kill_child_or_group(child: &mut Child, _kill_process_group: bool) {
    let _ = child.kill();
}

pub fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
    kill_process_group: bool,
) -> Result<CommandOutput, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_process_group(command, kill_process_group);
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to spawn command {:?}: {error}", command))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_child_or_group(&mut child, kill_process_group);
                    let output = child.wait_with_output().map_err(|error| {
                        format!("failed to collect timed out command output: {error}")
                    })?;
                    return Err(format!(
                        "command {:?} timed out after {:.2}s: {}",
                        command,
                        timeout.as_secs_f32(),
                        String::from_utf8_lossy(&output.stderr).trim()
                    ));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                kill_child_or_group(&mut child, kill_process_group);
                return Err(format!("failed waiting on command {:?}: {error}", command));
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to collect command output {:?}: {error}", command))?;
    Ok(CommandOutput {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

#[cfg(unix)]
unsafe fn libc_setpgid(pid: i32, pgid: i32) -> i32 {
    unsafe extern "C" {
        fn setpgid(pid: i32, pgid: i32) -> i32;
    }
    unsafe { setpgid(pid, pgid) }
}

#[cfg(unix)]
unsafe fn libc_kill(pid: i32, signal: i32) -> i32 {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    unsafe { kill(pid, signal) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_with_timeout_times_out() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("sleep 1");
        let error = run_command_with_timeout(&mut command, Duration::from_millis(50), true)
            .expect_err("sleep should time out");
        assert!(error.contains("timed out"));
    }

    #[test]
    fn run_command_with_timeout_captures_output() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("printf hello; printf oops >&2");
        let output = run_command_with_timeout(&mut command, Duration::from_secs(1), true)
            .expect("command should succeed");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "oops");
    }
}
