use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const LINUX_SUN_PATH_MAX_BYTES: usize = 107;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XdgPaths {
    pub config_home: PathBuf,
    pub data_home: PathBuf,
    pub state_home: PathBuf,
    pub cache_home: PathBuf,
    pub runtime_dir: Option<PathBuf>,
}

impl XdgPaths {
    pub fn resolve(
        environment: &HashMap<String, String>,
        home: &Path,
    ) -> Result<Self, RuntimePathError> {
        if !home.is_absolute() {
            return Err(RuntimePathError::NotAbsolute("home"));
        }
        Ok(Self {
            config_home: xdg_home(environment, "XDG_CONFIG_HOME", home.join(".config"))?,
            data_home: xdg_home(environment, "XDG_DATA_HOME", home.join(".local/share"))?,
            state_home: xdg_home(environment, "XDG_STATE_HOME", home.join(".local/state"))?,
            cache_home: xdg_home(environment, "XDG_CACHE_HOME", home.join(".cache"))?,
            runtime_dir: environment
                .get("XDG_RUNTIME_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .filter(|path| path.is_absolute()),
        })
    }

    #[must_use]
    pub fn settings_file(&self) -> PathBuf {
        self.config_home.join("kitmux/settings.json")
    }

    #[must_use]
    pub fn state_file(&self) -> PathBuf {
        self.state_home.join("kitmux/state.json")
    }
}

fn xdg_home(
    environment: &HashMap<String, String>,
    key: &'static str,
    fallback: PathBuf,
) -> Result<PathBuf, RuntimePathError> {
    let Some(value) = environment.get(key).filter(|value| !value.is_empty()) else {
        return Ok(fallback);
    };
    let path = PathBuf::from(value);
    path.is_absolute()
        .then_some(path)
        .ok_or(RuntimePathError::NotAbsolute(key))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnixSocketAddress {
    path: PathBuf,
}

impl UnixSocketAddress {
    pub fn resolve(
        environment: &HashMap<String, String>,
        xdg: &XdgPaths,
        uid: u32,
    ) -> Result<Self, RuntimePathError> {
        if let Some(value) = environment
            .get("KITMUX_SOCKET_PATH")
            .filter(|value| !value.is_empty())
        {
            return Self::new(PathBuf::from(value));
        }
        let preferred = xdg
            .runtime_dir
            .as_ref()
            .map(|root| root.join("kitmux/kitmux.sock"));
        if let Some(path) = preferred
            && path_bytes(&path)? <= LINUX_SUN_PATH_MAX_BYTES
        {
            return Self::new(path);
        }
        Self::new(PathBuf::from(format!("/tmp/kitmux-{uid}/kitmux.sock")))
    }

    pub fn new(path: PathBuf) -> Result<Self, RuntimePathError> {
        if !path.is_absolute() {
            return Err(RuntimePathError::NotAbsolute("socket"));
        }
        let length = path_bytes(&path)?;
        if length > LINUX_SUN_PATH_MAX_BYTES {
            return Err(RuntimePathError::SocketPathTooLong(length));
        }
        Ok(Self { path })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn prepare_parent(&self, expected_uid: u32) -> Result<(), RuntimePathError> {
        let parent = self.path.parent().ok_or(RuntimePathError::MissingParent)?;
        match fs::symlink_metadata(parent) {
            Ok(metadata) => validate_private_directory(&metadata, expected_uid),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(parent).map_err(RuntimePathError::Io)?;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                    .map_err(RuntimePathError::Io)?;
                let metadata = fs::symlink_metadata(parent).map_err(RuntimePathError::Io)?;
                validate_private_directory(&metadata, expected_uid)
            }
            Err(error) => Err(RuntimePathError::Io(error)),
        }
    }
}

fn validate_private_directory(
    metadata: &fs::Metadata,
    expected_uid: u32,
) -> Result<(), RuntimePathError> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RuntimePathError::UnsafeRuntimeDirectory);
    }
    if metadata.uid() != expected_uid {
        return Err(RuntimePathError::WrongOwner {
            expected: expected_uid,
            actual: metadata.uid(),
        });
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(RuntimePathError::UnsafePermissions(metadata.mode() & 0o777));
    }
    Ok(())
}

fn path_bytes(path: &Path) -> Result<usize, RuntimePathError> {
    let text = path.to_str().ok_or(RuntimePathError::NonUtf8)?;
    if text.as_bytes().contains(&0) {
        return Err(RuntimePathError::ContainsNul);
    }
    Ok(text.len())
}

#[derive(Debug)]
pub enum RuntimePathError {
    NotAbsolute(&'static str),
    SocketPathTooLong(usize),
    NonUtf8,
    ContainsNul,
    MissingParent,
    UnsafeRuntimeDirectory,
    UnsafePermissions(u32),
    WrongOwner { expected: u32, actual: u32 },
    Io(io::Error),
}

impl fmt::Display for RuntimePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAbsolute(name) => write!(f, "{name} path must be absolute"),
            Self::SocketPathTooLong(length) => {
                write!(f, "Unix socket path is {length} bytes; Linux allows 107")
            }
            Self::NonUtf8 => f.write_str("path is not UTF-8"),
            Self::ContainsNul => f.write_str("path contains NUL"),
            Self::MissingParent => f.write_str("path has no parent directory"),
            Self::UnsafeRuntimeDirectory => {
                f.write_str("runtime directory is not a real directory")
            }
            Self::UnsafePermissions(mode) => {
                write!(f, "runtime directory permissions {mode:o} are not private")
            }
            Self::WrongOwner { expected, actual } => {
                write!(
                    f,
                    "runtime directory owner {actual} does not match {expected}"
                )
            }
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for RuntimePathError {}

#[must_use]
pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path, maximum_bytes: u64) -> io::Result<String> {
    let path_metadata = fs::symlink_metadata(path)?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe file type",
        ));
    }
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > maximum_bytes {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "file exceeds bound",
        ));
    }
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > maximum_bytes {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "file exceeds bound",
            ));
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn read_bounded(path: &Path, maximum_bytes: u64) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe file type",
        ));
    }
    if metadata.len() > maximum_bytes {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "file exceeds bound",
        ));
    }
    let mut data = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?
        .take(maximum_bytes + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > maximum_bytes {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "file exceeds bound",
        ));
    }
    Ok(data)
}

#[derive(Debug)]
pub enum AtomicWriteError {
    MissingParent,
    UnsafeParent,
    UnsafeDestination,
    Io(io::Error),
}

impl fmt::Display for AtomicWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingParent => f.write_str("destination has no parent"),
            Self::UnsafeParent => f.write_str("destination parent is a symlink or non-directory"),
            Self::UnsafeDestination => f.write_str("destination is a symlink or non-file"),
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AtomicWriteError {}

pub fn atomic_write_private(path: &Path, data: &[u8]) -> Result<(), AtomicWriteError> {
    let parent = path.parent().ok_or(AtomicWriteError::MissingParent)?;
    fs::create_dir_all(parent).map_err(AtomicWriteError::Io)?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(AtomicWriteError::Io)?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(AtomicWriteError::UnsafeParent);
    }
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.uid() != parent_metadata.uid())
    {
        return Err(AtomicWriteError::UnsafeDestination);
    }
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".kitmux-write-{}-{sequence}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(AtomicWriteError::Io)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileFingerprint {
    pub length: u64,
    pub modified: Option<SystemTime>,
    pub sha256: String,
}

impl FileFingerprint {
    pub fn read(path: &Path, maximum_bytes: u64) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsafe file type",
            ));
        }
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            sha256: sha256_file(path, maximum_bytes)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileChange {
    Created,
    Modified,
    Removed,
}

pub struct PollingFileWatcher {
    path: PathBuf,
    maximum_bytes: u64,
    previous: Option<FileFingerprint>,
}

impl PollingFileWatcher {
    pub fn new(path: PathBuf, maximum_bytes: u64) -> io::Result<Self> {
        let previous = match FileFingerprint::read(&path, maximum_bytes) {
            Ok(value) => Some(value),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            path,
            maximum_bytes,
            previous,
        })
    }

    pub fn poll(&mut self) -> io::Result<Option<FileChange>> {
        let current = match FileFingerprint::read(&self.path, self.maximum_bytes) {
            Ok(value) => Some(value),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let change = match (&self.previous, &current) {
            (None, Some(_)) => Some(FileChange::Created),
            (Some(_), None) => Some(FileChange::Removed),
            (Some(before), Some(after)) if before != after => Some(FileChange::Modified),
            _ => None,
        };
        self.previous = current;
        Ok(change)
    }
}
