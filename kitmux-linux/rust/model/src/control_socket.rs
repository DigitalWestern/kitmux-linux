use crate::{
    ControlRequest, ControlResponse, RuntimePathError, UnixSocketAddress, decode_control_request,
    decode_control_response, encode_control_request, encode_control_response,
};
use serde::Serialize;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const CONTROL_MAX_CLIENTS: usize = 32;
pub const CONTROL_IO_TIMEOUT: Duration = Duration::from_secs(2);
pub const CONTROL_MAX_EVENT_HISTORY: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerCredentials {
    pub pid: i32,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug)]
pub enum ControlSocketError {
    Io(io::Error),
    UnsafeExistingPath,
    LiveServer,
    PeerCredentialsUnavailable,
    ServerStopped,
    Path(String),
}

impl fmt::Display for ControlSocketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::UnsafeExistingPath => formatter
                .write_str("socket path exists but is not a socket owned by the current user"),
            Self::LiveServer => formatter.write_str("another Kitmux control server is listening"),
            Self::PeerCredentialsUnavailable => {
                formatter.write_str("Linux peer credentials are unavailable")
            }
            Self::ServerStopped => formatter.write_str("control server stopped before dispatch"),
            Self::Path(error) => formatter.write_str(error),
        }
    }
}

impl std::error::Error for ControlSocketError {}

impl From<io::Error> for ControlSocketError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct ControlServer {
    path: std::path::PathBuf,
    identity: (u64, u64),
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ControlServer {
    pub fn start<F>(
        address: UnixSocketAddress,
        history: ControlEventHistory,
        handler: F,
    ) -> Result<Self, ControlSocketError>
    where
        F: Fn(ControlRequest, PeerCredentials) -> ControlResponse + Send + Sync + 'static,
    {
        let path = address.path().to_owned();
        let uid = unsafe { libc::geteuid() };
        address
            .prepare_parent(uid)
            .map_err(|error: RuntimePathError| ControlSocketError::Path(error.to_string()))?;
        let _path_lock = SocketPathLock::acquire(&path)?;
        remove_stale_socket(&path, uid)?;
        let listener = UnixListener::bind(&path)?;
        let identity = socket_identity(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        validate_bound_socket(&path, uid, identity)?;
        listener.set_nonblocking(true)?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handler = Arc::new(handler);
        let active_clients = Arc::new(AtomicUsize::new(0));
        let thread_history = history;
        let thread_path = path.clone();
        let thread = thread::Builder::new()
            .name("kitmux-control-accept".to_owned())
            .spawn(move || {
                accept_loop(
                    thread_path,
                    identity,
                    listener,
                    thread_stop,
                    handler,
                    active_clients,
                    thread_history,
                );
            })?;
        Ok(Self {
            path: address.path().to_owned(),
            identity,
            stop,
            thread: Some(thread),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

struct SocketPathLock(File);

impl SocketPathLock {
    fn acquire(path: &Path) -> Result<Self, ControlSocketError> {
        let lock_path = path.with_extension("lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(lock_path)?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if result != 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(Self(file))
    }
}

impl Drop for SocketPathLock {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if socket_identity(&self.path).ok() == Some(self.identity) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn accept_loop<F>(
    path: std::path::PathBuf,
    identity: (u64, u64),
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    handler: Arc<F>,
    active_clients: Arc<AtomicUsize>,
    history: ControlEventHistory,
) where
    F: Fn(ControlRequest, PeerCredentials) -> ControlResponse + Send + Sync + 'static,
{
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if active_clients.fetch_add(1, Ordering::AcqRel) >= CONTROL_MAX_CLIENTS {
                    active_clients.fetch_sub(1, Ordering::AcqRel);
                    history.record("<rejected>", "", false, unsafe { libc::geteuid() });
                    continue;
                }
                let handler = Arc::clone(&handler);
                let active_clients = Arc::clone(&active_clients);
                let history = history.clone();
                let _ = thread::Builder::new()
                    .name("kitmux-control-client".to_owned())
                    .spawn(move || {
                        serve_client(stream, handler, history);
                        active_clients.fetch_sub(1, Ordering::AcqRel);
                    });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    if socket_identity(&path).ok() == Some(identity) {
        let _ = fs::remove_file(path);
    }
}

fn serve_client<F>(mut stream: UnixStream, handler: Arc<F>, history: ControlEventHistory)
where
    F: Fn(ControlRequest, PeerCredentials) -> ControlResponse + Send + Sync + 'static,
{
    let _ = configure_no_sigpipe(stream.as_raw_fd());
    let _ = stream.set_read_timeout(Some(CONTROL_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CONTROL_IO_TIMEOUT));
    let response = match peer_credentials(stream.as_raw_fd()) {
        Ok(peer) if peer.uid == unsafe { libc::geteuid() } => {
            match read_frame(&mut stream, crate::CONTROL_MAX_REQUEST_BYTES, true) {
                Ok(frame) => match decode_control_request(&frame) {
                    Ok(request) => handler(request, peer),
                    Err(error) => {
                        let request_id = request_id_from_frame(&frame);
                        history.record("<rejected>", &request_id, false, peer.uid);
                        ControlResponse::failure(
                            request_id,
                            error.response_code(),
                            error.to_string(),
                        )
                    }
                },
                Err(error) => {
                    history.record("<rejected>", "", false, peer.uid);
                    ControlResponse::failure("", error.response_code(), error.to_string())
                }
            }
        }
        Ok(peer) => {
            history.record("<rejected>", "", false, peer.uid);
            ControlResponse::failure("", "unauthorized", "peer is not the current user")
        }
        Err(error) => {
            history.record("<rejected>", "", false, unsafe { libc::geteuid() });
            ControlResponse::failure("", "unauthorized", error.to_string())
        }
    };
    let response_id = response.id.clone();
    let data = match encode_control_response(&response) {
        Ok(mut data) => {
            data.push(b'\n');
            data
        }
        Err(error) => {
            let fallback =
                ControlResponse::failure(response_id, error.response_code(), error.to_string());
            let Ok(mut data) = encode_control_response(&fallback) else {
                return;
            };
            data.push(b'\n');
            data
        }
    };
    let _ = write_frame(&mut stream, &data);
}

pub fn send_control_request(
    address: &UnixSocketAddress,
    request: &ControlRequest,
) -> Result<ControlResponse, ControlClientError> {
    let mut stream = UnixStream::connect(address.path())?;
    configure_no_sigpipe(stream.as_raw_fd())?;
    stream.set_read_timeout(Some(CONTROL_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(CONTROL_IO_TIMEOUT))?;
    let mut data = encode_control_request(request).map_err(ControlClientError::Codec)?;
    data.push(b'\n');
    write_frame(&mut stream, &data)?;
    let frame = read_frame(&mut stream, crate::CONTROL_MAX_RESPONSE_BYTES, false)
        .map_err(ControlClientError::Codec)?;
    if frame.is_empty() {
        return Err(ControlClientError::EmptyResponse);
    }
    decode_control_response(&frame).map_err(ControlClientError::Codec)
}

#[derive(Debug)]
pub enum ControlClientError {
    Io(io::Error),
    Codec(crate::ControlCodecError),
    EmptyResponse,
}

impl fmt::Display for ControlClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Codec(error) => error.fmt(formatter),
            Self::EmptyResponse => formatter.write_str("server closed without a response"),
        }
    }
}

impl std::error::Error for ControlClientError {}

impl From<io::Error> for ControlClientError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

fn read_frame(
    stream: &mut UnixStream,
    maximum_bytes: usize,
    request: bool,
) -> Result<Vec<u8>, crate::ControlCodecError> {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| match error.kind() {
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                    crate::ControlCodecError::Timeout
                }
                _ => crate::ControlCodecError::IncompleteFrame,
            })?;
        if count == 0 {
            return if data.is_empty() {
                Ok(data)
            } else {
                Err(crate::ControlCodecError::IncompleteFrame)
            };
        }
        data.extend_from_slice(&buffer[..count]);
        if data.len() > maximum_bytes + 1 {
            return Err(if request {
                crate::ControlCodecError::RequestTooLarge
            } else {
                crate::ControlCodecError::ResponseTooLarge
            });
        }
        if let Some(newline) = data.iter().position(|byte| *byte == b'\n') {
            let frame_end = if newline > 0 && data[newline - 1] == b'\r' {
                newline - 1
            } else {
                newline
            };
            if frame_end > maximum_bytes {
                return Err(if request {
                    crate::ControlCodecError::RequestTooLarge
                } else {
                    crate::ControlCodecError::ResponseTooLarge
                });
            }
            if data[newline + 1..]
                .iter()
                .any(|byte| !matches!(*byte, b'\r' | b'\n' | b' ' | b'\t'))
            {
                return Err(crate::ControlCodecError::InvalidEnvelope);
            }
            return Ok(data[..frame_end].to_vec());
        }
        if data.len() > maximum_bytes {
            return Err(if request {
                crate::ControlCodecError::RequestTooLarge
            } else {
                crate::ControlCodecError::ResponseTooLarge
            });
        }
    }
}

fn request_id_from_frame(frame: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(frame)
        .ok()
        .and_then(|value| value.get("id")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn configure_no_sigpipe(fd: RawFd) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let enabled: libc::c_int = 1;
        let result = unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                (&enabled as *const libc::c_int).cast(),
                std::mem::size_of_val(&enabled) as libc::socklen_t,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = fd;
    Ok(())
}

fn write_frame(stream: &mut UnixStream, data: &[u8]) -> io::Result<()> {
    let fd = stream.as_raw_fd();
    let mut offset = 0;
    while offset < data.len() {
        #[cfg(target_os = "linux")]
        let flags = libc::MSG_NOSIGNAL;
        #[cfg(not(target_os = "linux"))]
        let flags = 0;
        let written = unsafe {
            libc::send(
                fd,
                data[offset..].as_ptr().cast(),
                data.len() - offset,
                flags,
            )
        };
        if written > 0 {
            offset += written as usize;
        } else if written < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        } else if written < 0 {
            return Err(io::Error::last_os_error());
        } else {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "socket write returned zero",
            ));
        }
    }
    Ok(())
}

fn remove_stale_socket(path: &Path, uid: u32) -> Result<(), ControlSocketError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_socket()
        || metadata.uid() != uid
    {
        return Err(ControlSocketError::UnsafeExistingPath);
    }
    if UnixStream::connect(path).is_ok() {
        return Err(ControlSocketError::LiveServer);
    }
    fs::remove_file(path)?;
    Ok(())
}

fn socket_identity(path: &Path) -> io::Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

fn validate_bound_socket(
    path: &Path,
    uid: u32,
    identity: (u64, u64),
) -> Result<(), ControlSocketError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_socket()
        || metadata.uid() != uid
        || (metadata.mode() & 0o777) != 0o600
        || socket_identity(path)? != identity
    {
        return Err(ControlSocketError::UnsafeExistingPath);
    }
    Ok(())
}

fn peer_credentials(fd: RawFd) -> Result<PeerCredentials, ControlSocketError> {
    #[cfg(target_os = "linux")]
    {
        let mut credentials = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut length,
            )
        };
        if result == 0 && length as usize >= std::mem::size_of::<libc::ucred>() {
            return Ok(PeerCredentials {
                pid: credentials.pid,
                uid: credentials.uid,
                gid: credentials.gid,
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = fd;
    Err(ControlSocketError::PeerCredentialsUnavailable)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ControlEvent {
    pub cursor: u64,
    pub method: String,
    pub request_id: String,
    pub ok: bool,
    pub peer_uid: u32,
    pub monotonic_ms: u64,
}

#[derive(Clone)]
pub struct ControlEventHistory {
    events: Arc<std::sync::Mutex<std::collections::VecDeque<ControlEvent>>>,
    next_cursor: Arc<AtomicUsize>,
    started_at: Arc<Instant>,
}

impl Default for ControlEventHistory {
    fn default() -> Self {
        Self {
            events: Arc::default(),
            next_cursor: Arc::default(),
            started_at: Arc::new(Instant::now()),
        }
    }
}

impl ControlEventHistory {
    pub fn record(&self, method: &str, request_id: &str, ok: bool, peer_uid: u32) {
        let cursor = self.next_cursor.fetch_add(1, Ordering::Relaxed) as u64 + 1;
        let mut events = self
            .events
            .lock()
            .expect("control event history lock poisoned");
        events.push_back(ControlEvent {
            cursor,
            method: method.to_owned(),
            request_id: request_id.to_owned(),
            ok,
            peer_uid,
            monotonic_ms: self.started_at.elapsed().as_millis() as u64,
        });
        while events.len() > CONTROL_MAX_EVENT_HISTORY {
            events.pop_front();
        }
    }

    #[must_use]
    pub fn list(&self, after: u64, limit: usize, category: Option<&str>) -> Vec<ControlEvent> {
        self.events
            .lock()
            .expect("control event history lock poisoned")
            .iter()
            .filter(|event| {
                event.cursor > after
                    && category.is_none_or(|category| event.method.starts_with(category))
            })
            .take(limit.min(500))
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn cursor(&self) -> u64 {
        self.next_cursor.load(Ordering::Relaxed) as u64
    }
}
