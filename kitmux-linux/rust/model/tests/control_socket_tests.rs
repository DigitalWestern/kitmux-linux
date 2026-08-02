#![cfg(target_os = "linux")]

use kitmux_model::{
    CONTROL_MAX_CLIENTS, CONTROL_MAX_REQUEST_BYTES, ControlEventHistory, ControlMethod,
    ControlRequest, ControlResponse, ControlServer, ControlSocketError, RuntimePathError,
    UnixSocketAddress, decode_control_response, send_control_request,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

struct TestServer {
    root: PathBuf,
    address: UnixSocketAddress,
    server: Option<ControlServer>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.server.take();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn root() -> PathBuf {
    let base = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let name = format!("k-{}-{}", std::process::id(), Uuid::new_v4().simple());
    let candidate = base.join(&name);
    // ponytail: fall back to /tmp when an inherited runtime path cannot fit a
    // Unix sun_path; the real runtime resolver already rejects that path.
    let root = if candidate.join("s").to_string_lossy().len() <= 100 {
        candidate
    } else {
        PathBuf::from("/tmp").join(name)
    };
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    root
}

fn address(root: &Path) -> UnixSocketAddress {
    UnixSocketAddress::new(root.join("kitmux.sock")).unwrap()
}

fn handler(request: ControlRequest, _peer: kitmux_model::PeerCredentials) -> ControlResponse {
    ControlResponse::success(request.id, json!({"method": request.method}))
}

fn start(root: PathBuf) -> TestServer {
    let address = address(&root);
    let server =
        ControlServer::start(address.clone(), ControlEventHistory::default(), handler).unwrap();
    TestServer {
        root,
        address,
        server: Some(server),
    }
}

fn request(method: ControlMethod) -> ControlRequest {
    ControlRequest {
        version: 1,
        id: format!("test-{}", Uuid::new_v4()),
        method: method.as_str().to_owned(),
        params: BTreeMap::new(),
        context: None,
    }
}

fn raw_response(address: &UnixSocketAddress, data: &[u8]) -> ControlResponse {
    let mut stream = UnixStream::connect(address.path()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(6)))
        .unwrap();
    stream.write_all(data).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    decode_control_response(response.strip_suffix(b"\n").unwrap_or(&response)).unwrap()
}

#[test]
fn round_trip_and_socket_identity_are_real_filesystem_properties() {
    let running = start(root());
    let response = send_control_request(&running.address, &request(ControlMethod::Ping)).unwrap();
    assert!(response.ok);
    assert_eq!(response.result.unwrap()["method"], "ping");

    let metadata = fs::symlink_metadata(running.address.path()).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert_eq!(metadata.mode() & 0o777, 0o600);
}

#[test]
fn stale_socket_is_replaced_and_live_socket_is_refused() {
    let root = root();
    let address = address(&root);
    let stale = UnixListener::bind(address.path()).unwrap();
    drop(stale);
    assert!(address.path().exists());

    let server =
        ControlServer::start(address.clone(), ControlEventHistory::default(), handler).unwrap();
    let live = ControlServer::start(address.clone(), ControlEventHistory::default(), handler);
    assert!(matches!(live, Err(ControlSocketError::LiveServer)));
    drop(server);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn symlink_socket_path_is_rejected_and_preserved() {
    let root = root();
    let address = address(&root);
    let target = root.join("target");
    fs::write(&target, b"keep").unwrap();
    symlink(&target, address.path()).unwrap();

    let result = ControlServer::start(address.clone(), ControlEventHistory::default(), handler);
    assert!(matches!(
        result,
        Err(ControlSocketError::UnsafeExistingPath)
    ));
    assert!(
        fs::symlink_metadata(address.path())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_and_oversized_frames_get_protocol_errors() {
    let running = start(root());
    let malformed = raw_response(&running.address, b"{\n");
    assert_eq!(malformed.error.unwrap().code, "malformed_request");

    let mut oversized = vec![b'x'; CONTROL_MAX_REQUEST_BYTES + 1];
    oversized.push(b'\n');
    let oversized = raw_response(&running.address, &oversized);
    assert_eq!(oversized.error.unwrap().code, "request_too_large");
}

#[test]
fn drop_does_not_remove_a_successor_socket() {
    let root = root();
    let address = address(&root);
    let first =
        ControlServer::start(address.clone(), ControlEventHistory::default(), handler).unwrap();
    let moved = root.join("first.sock");
    fs::rename(address.path(), &moved).unwrap();
    let second =
        ControlServer::start(address.clone(), ControlEventHistory::default(), handler).unwrap();
    drop(first);
    assert!(address.path().exists());
    drop(second);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn simultaneous_starts_have_one_winner_and_keep_its_socket() {
    let root = root();
    let address = address(&root);
    let barrier = Arc::new(Barrier::new(2));
    let first_barrier = Arc::clone(&barrier);
    let first_address = address.clone();
    let first = thread::spawn(move || {
        first_barrier.wait();
        ControlServer::start(first_address, ControlEventHistory::default(), handler)
    });
    let second_barrier = Arc::clone(&barrier);
    let second_address = address.clone();
    let second = thread::spawn(move || {
        second_barrier.wait();
        ControlServer::start(second_address, ControlEventHistory::default(), handler)
    });
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    let winners = [first, second]
        .into_iter()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert_eq!(winners.len(), 1);
    assert!(address.path().exists());
    drop(winners);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn world_writable_grandparent_is_rejected() {
    let root = root();
    let grandparent = root.join("w");
    let parent = grandparent.join("k");
    fs::create_dir_all(&parent).unwrap();
    fs::set_permissions(&grandparent, fs::Permissions::from_mode(0o777)).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = address(&parent);
    assert!(matches!(
        socket.prepare_parent(unsafe { libc::geteuid() }),
        Err(RuntimePathError::UnsafePermissions(0o777))
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn client_cap_returns_busy_instead_of_eof() {
    let running = start(root());
    let mut held = Vec::new();
    for _ in 0..CONTROL_MAX_CLIENTS {
        held.push(UnixStream::connect(running.address.path()).unwrap());
    }
    thread::sleep(Duration::from_millis(200));
    let response = raw_response(
        &running.address,
        b"{\"version\":1,\"id\":\"cap\",\"method\":\"ping\"}\n",
    );
    assert_eq!(response.error.unwrap().code, "busy");
}

#[test]
fn dribbling_client_times_out_and_server_still_serves() {
    let running = start(root());
    let mut stream = UnixStream::connect(running.address.path()).unwrap();
    stream.write_all(b"{").unwrap();
    thread::sleep(Duration::from_secs(3));
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let response =
        decode_control_response(response.strip_suffix(b"\n").unwrap_or(&response)).unwrap();
    assert_eq!(response.error.unwrap().code, "timeout");
    assert!(
        send_control_request(&running.address, &request(ControlMethod::Ping))
            .unwrap()
            .ok
    );
}
