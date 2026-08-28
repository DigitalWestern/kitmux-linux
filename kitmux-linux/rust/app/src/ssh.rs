use kitmux_model::{SshProfile, SshResolution};
use serde_json::json;
use std::env;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::runtime::is_executable;

pub(crate) const SSH_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const SSH_RESOLUTION_MAX_OUTPUT: usize = 512 * 1024;

#[derive(Debug)]
pub(crate) enum SshRuntimeError {
    ExecutableNotFound,
    Launch,
    TimedOut,
    OutputTooLarge,
    CommandFailed,
    InvalidOutput,
}

impl std::fmt::Display for SshRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ExecutableNotFound => "the user's ssh executable was not found on PATH",
            Self::Launch => "ssh -G could not be launched",
            Self::TimedOut => "ssh -G timed out",
            Self::OutputTooLarge => "ssh -G returned too much output",
            Self::CommandFailed => "ssh -G failed",
            Self::InvalidOutput => "ssh -G returned incomplete output",
        })
    }
}

pub(crate) fn find_ssh_executable() -> Result<PathBuf, SshRuntimeError> {
    let path = env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"));
    env::split_paths(&path)
        .map(|directory| directory.join("ssh"))
        .find_map(|candidate| {
            (candidate.is_file() && is_executable(&candidate))
                .then(|| std::fs::canonicalize(candidate).ok())
                .flatten()
        })
        .ok_or(SshRuntimeError::ExecutableNotFound)
}

pub(crate) fn read_limited(mut reader: impl Read) -> (Vec<u8>, bool) {
    let mut output = Vec::new();
    let mut too_large = false;
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if output.len() < SSH_RESOLUTION_MAX_OUTPUT {
                    let keep = count.min(SSH_RESOLUTION_MAX_OUTPUT - output.len());
                    output.extend_from_slice(&buffer[..keep]);
                    if keep < count {
                        too_large = true;
                    }
                } else {
                    too_large = true;
                }
            }
            Err(_) => break,
        }
    }
    (output, too_large)
}

pub(crate) fn run_ssh_resolution(
    executable: &Path,
    host_alias: &str,
) -> Result<Vec<u8>, SshRuntimeError> {
    let mut child = Command::new(executable)
        .args(["-G", "--", host_alias])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| SshRuntimeError::Launch)?;
    let stdout = child.stdout.take().ok_or(SshRuntimeError::Launch)?;
    let stderr = child.stderr.take().ok_or(SshRuntimeError::Launch)?;
    let stdout_reader = thread::spawn(move || read_limited(stdout));
    let stderr_reader = thread::spawn(move || read_limited(stderr));
    let deadline = Instant::now() + SSH_RESOLUTION_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(SshRuntimeError::TimedOut);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(SshRuntimeError::Launch);
            }
        }
    };
    let (output, output_too_large) = stdout_reader.join().unwrap_or((Vec::new(), false));
    let (_, error_too_large) = stderr_reader.join().unwrap_or((Vec::new(), false));
    if output_too_large || error_too_large {
        return Err(SshRuntimeError::OutputTooLarge);
    }
    status
        .success()
        .then_some(output)
        .ok_or(SshRuntimeError::CommandFailed)
}

pub(crate) fn resolve_ssh_profile(
    profile: &SshProfile,
) -> Result<(PathBuf, SshResolution), SshRuntimeError> {
    let executable = find_ssh_executable()?;
    let output = run_ssh_resolution(&executable, &profile.host_alias)?;
    let text = String::from_utf8(output).map_err(|_| SshRuntimeError::InvalidOutput)?;
    let resolution =
        SshResolution::parse(&profile.host_alias, &text).ok_or(SshRuntimeError::InvalidOutput)?;
    Ok((executable, resolution))
}

pub(crate) fn ssh_review_json(review: &kitmux_model::SshConnectionReview) -> serde_json::Value {
    json!({
        "fingerprint": review.fingerprint,
        "destination": review.destination,
        "hostAlias": review.host_alias,
        "remoteCommand": review.remote_command,
        "strictHostKeyChecking": review.strict_host_key_checking,
        "proxyJump": review.proxy_jump,
        "proxyCommand": review.proxy_command,
        "forwards": review.forwards,
        "hasExternallyListeningForward": review.has_externally_listening_forward,
        "requiresApproval": review.requires_approval,
    })
}

pub(crate) fn disconnected_ssh_argv(profile_id: Uuid) -> Vec<OsString> {
    vec![
        OsString::from("/usr/bin/printf"),
        OsString::from("%s\\n"),
        OsString::from(format!(
            "SSH profile {profile_id} restored disconnected; use explicit reconnect."
        )),
    ]
}
