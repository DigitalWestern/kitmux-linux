use crate::settings::SETTINGS_MAX_BYTES;
use crate::{
    AppSnapshot, SNAPSHOT_MAX_BYTES, SettingsCodecError, SettingsDocument, SnapshotCodecError,
    atomic_write_private, decode_settings, decode_snapshot, encode_settings, encode_snapshot,
    read_bounded,
};
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ASIDE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadDisposition {
    Missing,
    Loaded,
    SetAside(PathBuf),
    RecoveredFromLastGood,
    Unreadable,
}

pub struct SettingsLoad {
    pub document: SettingsDocument,
    pub disposition: LoadDisposition,
    pub may_write: bool,
}

pub struct StateLoad {
    pub snapshot: Option<AppSnapshot>,
    pub disposition: LoadDisposition,
    pub may_write: bool,
}

#[must_use]
pub fn load_settings_at_launch(path: &Path) -> SettingsLoad {
    let defaults = || decode_settings(b"{}").expect("empty settings are valid");
    let bytes = match read_private(path, SETTINGS_MAX_BYTES as u64) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            return SettingsLoad {
                document: defaults(),
                disposition: LoadDisposition::Missing,
                may_write: true,
            };
        }
        Err(_) => {
            return SettingsLoad {
                document: defaults(),
                disposition: LoadDisposition::Unreadable,
                may_write: false,
            };
        }
    };
    match decode_settings(&bytes) {
        Ok(document) => SettingsLoad {
            document,
            disposition: LoadDisposition::Loaded,
            may_write: true,
        },
        Err(error) => {
            let label = match error {
                SettingsCodecError::UnsupportedVersion(version) => format!("v{version}-backup"),
                SettingsCodecError::TooLarge | SettingsCodecError::Malformed => {
                    "corrupt".to_owned()
                }
            };
            match set_aside(path, &label) {
                Ok(destination) => SettingsLoad {
                    document: defaults(),
                    disposition: LoadDisposition::SetAside(destination),
                    may_write: true,
                },
                Err(_) => SettingsLoad {
                    document: defaults(),
                    disposition: LoadDisposition::Unreadable,
                    may_write: false,
                },
            }
        }
    }
}

pub fn reload_settings(path: &Path) -> Option<SettingsDocument> {
    let bytes = read_private(path, SETTINGS_MAX_BYTES as u64).ok()??;
    decode_settings(&bytes).ok()
}

pub fn save_settings(path: &Path, document: &SettingsDocument) -> Result<(), String> {
    let bytes = encode_settings(document).map_err(|error| error.to_string())?;
    atomic_write_private(path, &bytes).map_err(|error| error.to_string())
}

#[must_use]
pub fn load_state_at_launch(path: &Path) -> StateLoad {
    let primary = read_private(path, SNAPSHOT_MAX_BYTES as u64);
    match primary {
        Ok(Some(bytes)) => match decode_snapshot(&bytes) {
            Ok(snapshot) => StateLoad {
                snapshot: Some(snapshot),
                disposition: LoadDisposition::Loaded,
                may_write: true,
            },
            Err(error) => {
                let label = match error {
                    SnapshotCodecError::UnsupportedVersion(version) => {
                        format!("v{version}-backup")
                    }
                    SnapshotCodecError::TooLarge
                    | SnapshotCodecError::Malformed
                    | SnapshotCodecError::Invalid(_) => "corrupt".to_owned(),
                };
                let aside = set_aside(path, &label);
                let (snapshot, recovered) = load_last_good(path);
                StateLoad {
                    snapshot,
                    disposition: if recovered {
                        LoadDisposition::RecoveredFromLastGood
                    } else {
                        aside.as_ref().map_or(LoadDisposition::Unreadable, |path| {
                            LoadDisposition::SetAside(path.clone())
                        })
                    },
                    may_write: aside.is_ok(),
                }
            }
        },
        Ok(None) => {
            let (snapshot, recovered) = load_last_good(path);
            StateLoad {
                snapshot,
                disposition: if recovered {
                    LoadDisposition::RecoveredFromLastGood
                } else {
                    LoadDisposition::Missing
                },
                may_write: true,
            }
        }
        Err(_) => {
            let (snapshot, recovered) = load_last_good(path);
            StateLoad {
                snapshot,
                disposition: if recovered {
                    LoadDisposition::RecoveredFromLastGood
                } else {
                    LoadDisposition::Unreadable
                },
                may_write: false,
            }
        }
    }
}

pub fn save_state(path: &Path, snapshot: AppSnapshot) -> Result<(), String> {
    let bytes = encode_snapshot(snapshot).map_err(|error| error.to_string())?;
    let last_good =
        match read_private(path, SNAPSHOT_MAX_BYTES as u64).map_err(|error| error.to_string())? {
            Some(previous) => {
                decode_snapshot(&previous).map_err(|error| error.to_string())?;
                previous
            }
            None => bytes.clone(),
        };
    atomic_write_private(&last_good_path(path), &last_good).map_err(|error| error.to_string())?;
    atomic_write_private(path, &bytes).map_err(|error| error.to_string())?;
    Ok(())
}

fn load_last_good(path: &Path) -> (Option<AppSnapshot>, bool) {
    let snapshot = read_private(&last_good_path(path), SNAPSHOT_MAX_BYTES as u64)
        .ok()
        .flatten()
        .and_then(|bytes| decode_snapshot(&bytes).ok());
    let recovered = snapshot.is_some();
    (snapshot, recovered)
}

#[must_use]
pub fn last_good_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.last-good", path.as_os_str().to_string_lossy()))
}

fn read_private(path: &Path, maximum_bytes: u64) -> io::Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file is owned by another user",
        ));
    }
    read_bounded(path, maximum_bytes).map(Some)
}

fn set_aside(path: &Path, label: &str) -> io::Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    set_aside_at(path, label, stamp)
}

fn set_aside_at(path: &Path, label: &str, stamp: u64) -> io::Result<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    loop {
        let sequence = ASIDE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let destination = path.with_file_name(format!("{file_name}.{label}-{stamp}-{sequence}"));
        match fs::hard_link(path, &destination) {
            Ok(()) => {
                fs::remove_file(path)?;
                return Ok(destination);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_aside_never_overwrites_an_existing_quarantine() {
        let root = std::env::temp_dir().join(format!(
            "kitmux-set-aside-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(&path, b"new-corrupt-input").unwrap();
        let sequence = ASIDE_SEQUENCE.load(Ordering::Relaxed);
        let collision = root.join(format!("settings.json.corrupt-7-{sequence}"));
        fs::write(&collision, b"prior-corrupt-input").unwrap();

        let destination = set_aside_at(&path, "corrupt", 7).unwrap();
        assert_ne!(destination, collision);
        assert_eq!(fs::read(&collision).unwrap(), b"prior-corrupt-input");
        assert_eq!(fs::read(&destination).unwrap(), b"new-corrupt-input");

        fs::remove_dir_all(root).unwrap();
    }
}
