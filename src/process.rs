use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// Run an argument-vector command with a deadline and capped captured output.
/// No shell is involved, including for user-supplied paths and URLs.
pub fn output(command: &mut Command, timeout: Duration, limit: u64) -> Result<Vec<u8>, String> {
    output_with_input(command, &[], timeout, limit)
}

/// Input is sent over a pipe, never through argv, environment variables or a shell.
pub fn output_with_input(
    command: &mut Command,
    input: &[u8],
    timeout: Duration,
    limit: u64,
) -> Result<Vec<u8>, String> {
    output_with_input_cancellable(command, input, timeout, limit, || false)
}

pub fn output_with_input_cancellable(
    command: &mut Command,
    input: &[u8],
    timeout: Duration,
    limit: u64,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<u8>, String> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", command.get_program().to_string_lossy()))?;
    let mut stdin = child.stdin.take().ok_or("Missing process input")?;
    let input = input.to_vec();
    let writer = thread::spawn(move || stdin.write_all(&input));
    let stdout = child.stdout.take().ok_or("Missing process output")?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let start = Instant::now();
    let status = loop {
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            break Err("Cancelled".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if start.elapsed() < timeout => thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("The request timed out".to_string());
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "Could not read process output")?
        .map_err(|e| e.to_string())?;
    let _ = writer.join();
    let status = status?;
    if bytes.len() as u64 > limit {
        return Err("Response exceeded the size limit".into());
    }
    if !status.success() {
        return Err(format!(
            "{} could not read this media",
            command.get_program().to_string_lossy()
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_terminates_a_stalled_helper() {
        let start = Instant::now();
        let result = output_with_input_cancellable(
            Command::new("sleep").arg("30"),
            &[],
            Duration::from_secs(40),
            1024,
            || start.elapsed() > Duration::from_millis(100),
        );
        assert_eq!(result.unwrap_err(), "Cancelled");
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
