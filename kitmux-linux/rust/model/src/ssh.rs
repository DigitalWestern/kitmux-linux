use crate::atomic_write_private;
use crate::sha256_bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const SSH_DOCUMENT_VERSION: i64 = 1;
pub const SSH_DOCUMENT_MAX_BYTES: usize = 1024 * 1024;
pub const SSH_RESOLUTION_MAX_BYTES: usize = 512 * 1024;
pub const SSH_PROFILE_MAX_COUNT: usize = 1000;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshProfile {
    pub id: Uuid,
    pub name: String,
    pub host_alias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_fingerprint: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SshProfileDocument {
    pub version: i64,
    pub profiles: Vec<SshProfile>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SshForwardKind {
    Local,
    Remote,
    Dynamic,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshForward {
    pub kind: SshForwardKind,
    pub value: String,
    pub externally_listening: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshResolution {
    pub host_alias: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub strict_host_key_checking: String,
    pub proxy_jump: Option<String>,
    pub proxy_command: Option<String>,
    pub forwards: Vec<SshForward>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionReview {
    pub fingerprint: String,
    pub destination: String,
    pub host_alias: String,
    pub remote_command: Option<String>,
    pub strict_host_key_checking: String,
    pub proxy_jump: Option<String>,
    pub proxy_command: Option<String>,
    pub forwards: Vec<SshForward>,
    pub has_externally_listening_forward: bool,
    pub requires_approval: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SshArgvError {
    InvalidProfile,
    InvalidExecutable,
}

impl fmt::Display for SshArgvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProfile => formatter.write_str("invalid SSH profile"),
            Self::InvalidExecutable => formatter.write_str("SSH executable must be absolute"),
        }
    }
}

impl std::error::Error for SshArgvError {}

#[derive(Debug)]
pub enum SshProfileStoreError {
    Io(String),
    InvalidPath,
    InvalidProfile,
    DuplicateName,
    MissingProfile,
    InvalidDocument,
    Codec(SshCodecError),
}

impl fmt::Display for SshProfileStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => formatter.write_str(error),
            Self::InvalidPath => formatter.write_str("SSH profile path is not a private file"),
            Self::InvalidProfile => formatter.write_str("invalid SSH profile"),
            Self::DuplicateName => formatter.write_str("SSH profile name already exists"),
            Self::MissingProfile => formatter.write_str("SSH profile was not found"),
            Self::InvalidDocument => formatter.write_str("SSH profile document is invalid"),
            Self::Codec(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SshProfileStoreError {}

pub struct SshProfileStore {
    path: PathBuf,
    document: Mutex<SshProfileDocument>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SshCodecError {
    TooLarge,
    Malformed,
    UnsupportedVersion(i64),
    Invalid(&'static str),
}

impl fmt::Display for SshCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => formatter.write_str("SSH document exceeds 1 MiB"),
            Self::Malformed => formatter.write_str("SSH document is not valid JSON"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported SSH document version {version}")
            }
            Self::Invalid(reason) => write!(formatter, "invalid SSH document: {reason}"),
        }
    }
}

impl std::error::Error for SshCodecError {}

pub fn decode_ssh_profiles(data: &[u8]) -> Result<SshProfileDocument, SshCodecError> {
    if data.len() > SSH_DOCUMENT_MAX_BYTES {
        return Err(SshCodecError::TooLarge);
    }
    let document: SshProfileDocument =
        serde_json::from_slice(data).map_err(|_| SshCodecError::Malformed)?;
    validate_ssh_profiles(document)
}

pub fn encode_ssh_profiles(document: SshProfileDocument) -> Result<Vec<u8>, SshCodecError> {
    let document = validate_ssh_profiles(document)?;
    let bytes = serde_json::to_vec_pretty(&document).map_err(|_| SshCodecError::Malformed)?;
    if bytes.len() > SSH_DOCUMENT_MAX_BYTES {
        return Err(SshCodecError::TooLarge);
    }
    Ok(bytes)
}

fn validate_ssh_profiles(
    mut document: SshProfileDocument,
) -> Result<SshProfileDocument, SshCodecError> {
    if document.version > SSH_DOCUMENT_VERSION {
        return Err(SshCodecError::UnsupportedVersion(document.version));
    }
    if document.profiles.len() > SSH_PROFILE_MAX_COUNT {
        return Err(SshCodecError::Invalid("too many profiles"));
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for profile in &mut document.profiles {
        if !ids.insert(profile.id) {
            return Err(SshCodecError::Invalid("duplicate profile ID"));
        }
        profile.name =
            safe_text(&profile.name, 128).ok_or(SshCodecError::Invalid("invalid profile name"))?;
        if !names.insert(profile.name.to_lowercase()) {
            return Err(SshCodecError::Invalid("duplicate profile name"));
        }
        profile.host_alias = safe_host_alias(&profile.host_alias)
            .ok_or(SshCodecError::Invalid("invalid host alias"))?;
        if !valid_timestamp(&profile.created_at)
            || !valid_timestamp(&profile.updated_at)
            || profile.updated_at < profile.created_at
        {
            return Err(SshCodecError::Invalid("invalid timestamps"));
        }
        if let Some(command) = profile.remote_command.take() {
            profile.remote_command = Some(
                safe_text(&command, 2048)
                    .ok_or(SshCodecError::Invalid("invalid remote command"))?,
            );
        }
        if profile
            .reviewed_fingerprint
            .as_deref()
            .is_some_and(|value| {
                value.len() != 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
        {
            profile.reviewed_fingerprint = None;
        }
    }
    document.version = SSH_DOCUMENT_VERSION;
    Ok(document)
}

impl SshResolution {
    pub fn argv(executable: &Path, profile: &SshProfile) -> Result<Vec<OsString>, SshArgvError> {
        if !executable.is_absolute() {
            return Err(SshArgvError::InvalidExecutable);
        }
        if safe_text(&profile.name, 128).is_none()
            || safe_host_alias(&profile.host_alias).is_none()
            || profile
                .remote_command
                .as_deref()
                .is_some_and(|command| safe_text(command, 2048).is_none())
            || !valid_timestamp(&profile.created_at)
            || !valid_timestamp(&profile.updated_at)
            || profile.updated_at < profile.created_at
        {
            return Err(SshArgvError::InvalidProfile);
        }
        let mut argv = vec![
            executable.as_os_str().to_owned(),
            OsString::from("--"),
            OsString::from(&profile.host_alias),
        ];
        if let Some(command) = &profile.remote_command {
            argv.push(OsString::from(command));
        }
        Ok(argv)
    }

    #[must_use]
    pub fn parse(host_alias: &str, output: &str) -> Option<Self> {
        if output.len() > SSH_RESOLUTION_MAX_BYTES {
            return None;
        }
        let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for line in output.lines() {
            let Some((key, value)) = line.split_once(' ') else {
                continue;
            };
            let value = value.trim();
            if key.is_empty() || value.is_empty() || value.len() > 8192 {
                continue;
            }
            values
                .entry(key.to_lowercase())
                .or_default()
                .push(value.to_owned());
        }
        let hostname = last(&values, "hostname")?.to_owned();
        let user = last(&values, "user")?.to_owned();
        let port = last(&values, "port")?
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)?;
        let strict_host_key_checking = last(&values, "stricthostkeychecking")
            .unwrap_or("ask")
            .to_owned();
        let gateway_ports = last(&values, "gatewayports").unwrap_or("no").to_lowercase();
        let mut forwards = Vec::new();
        append_forwards(
            &mut forwards,
            &values,
            "localforward",
            SshForwardKind::Local,
            &gateway_ports,
        );
        append_forwards(
            &mut forwards,
            &values,
            "remoteforward",
            SshForwardKind::Remote,
            &gateway_ports,
        );
        append_forwards(
            &mut forwards,
            &values,
            "dynamicforward",
            SshForwardKind::Dynamic,
            &gateway_ports,
        );
        Some(Self {
            host_alias: host_alias.to_owned(),
            hostname,
            user,
            port,
            strict_host_key_checking,
            proxy_jump: optional(&values, "proxyjump"),
            proxy_command: optional(&values, "proxycommand"),
            forwards,
        })
    }

    #[must_use]
    pub fn review(&self, profile: &SshProfile) -> SshConnectionReview {
        let port = self.port.to_string();
        let fields = [
            self.host_alias.as_str(),
            self.hostname.as_str(),
            self.user.as_str(),
            port.as_str(),
            self.strict_host_key_checking.as_str(),
            self.proxy_jump.as_deref().unwrap_or(""),
            self.proxy_command.as_deref().unwrap_or(""),
            profile.remote_command.as_deref().unwrap_or(""),
        ];
        let mut fingerprint_fields: Vec<String> = fields.into_iter().map(str::to_owned).collect();
        fingerprint_fields.extend(self.forwards.iter().map(|forward| {
            format!(
                "{}:{}:{}",
                match forward.kind {
                    SshForwardKind::Local => "local",
                    SshForwardKind::Remote => "remote",
                    SshForwardKind::Dynamic => "dynamic",
                },
                forward.value,
                forward.externally_listening
            )
        }));
        let fingerprint = sha256_bytes(fingerprint_fields.join("\0").as_bytes());
        let has_externally_listening_forward = self
            .forwards
            .iter()
            .any(|forward| forward.externally_listening);
        SshConnectionReview {
            fingerprint: fingerprint.clone(),
            destination: format!("{}@{}:{}", self.user, self.hostname, self.port),
            host_alias: self.host_alias.clone(),
            remote_command: profile.remote_command.clone(),
            strict_host_key_checking: self.strict_host_key_checking.clone(),
            proxy_jump: self.proxy_jump.clone(),
            proxy_command: self.proxy_command.clone(),
            forwards: self.forwards.clone(),
            has_externally_listening_forward,
            requires_approval: has_externally_listening_forward
                || profile.reviewed_fingerprint.as_deref() != Some(&fingerprint),
        }
    }
}

fn safe_text(raw: &str, maximum_bytes: usize) -> Option<String> {
    let value = raw.trim();
    (!value.is_empty()
        && value.len() <= maximum_bytes
        && !value
            .chars()
            .any(|character| character <= '\u{1f}' || character == '\u{7f}'))
    .then(|| value.to_owned())
}

fn safe_host_alias(raw: &str) -> Option<String> {
    let value = raw.trim();
    (!value.is_empty()
        && value.len() <= 255
        && !value.starts_with('-')
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control()))
    .then(|| value.to_owned())
}

fn valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20 {
        return false;
    }
    if !(bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        }))
    {
        return false;
    }
    let number = |range: std::ops::Range<usize>| {
        std::str::from_utf8(&bytes[range]).ok()?.parse::<u32>().ok()
    };
    let (year, month, day, hour, minute, second) = (
        number(0..4),
        number(5..7),
        number(8..10),
        number(11..13),
        number(14..16),
        number(17..19),
    );
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) =
        (year, month, day, hour, minute, second)
    else {
        return false;
    };
    let maximum_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => return false,
    };
    year > 0 && (1..=maximum_day).contains(&day) && hour <= 23 && minute <= 59 && second <= 59
}

fn last<'a>(values: &'a BTreeMap<String, Vec<String>>, key: &str) -> Option<&'a str> {
    values.get(key)?.last().map(String::as_str)
}

fn optional(values: &BTreeMap<String, Vec<String>>, key: &str) -> Option<String> {
    last(values, key)
        .filter(|value| !value.eq_ignore_ascii_case("none"))
        .map(str::to_owned)
}

fn append_forwards(
    result: &mut Vec<SshForward>,
    values: &BTreeMap<String, Vec<String>>,
    key: &str,
    kind: SshForwardKind,
    gateway_ports: &str,
) {
    for value in values.get(key).into_iter().flatten() {
        let lower = value.to_lowercase();
        let externally_listening = kind != SshForwardKind::Remote
            && (gateway_ports != "no"
                || lower.starts_with("*:")
                || lower.starts_with("0.0.0.0:")
                || lower.starts_with("[::]:")
                || lower.starts_with(":::"));
        result.push(SshForward {
            kind,
            value: value.clone(),
            externally_listening,
        });
    }
}

impl SshProfileStore {
    pub fn open(path: PathBuf) -> Result<Self, SshProfileStoreError> {
        let document = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(SshProfileStoreError::InvalidPath);
                }
                if metadata.uid() != unsafe { libc::geteuid() } {
                    return Err(SshProfileStoreError::InvalidPath);
                }
                if metadata.mode() & 0o077 != 0 {
                    return Err(SshProfileStoreError::InvalidPath);
                }
                let data =
                    fs::read(&path).map_err(|error| SshProfileStoreError::Io(error.to_string()))?;
                match decode_ssh_profiles(&data) {
                    Ok(document) => document,
                    Err(_) => {
                        quarantine_profile_file(&path)?;
                        SshProfileDocument {
                            version: SSH_DOCUMENT_VERSION,
                            profiles: Vec::new(),
                            extra: BTreeMap::new(),
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => SshProfileDocument {
                version: SSH_DOCUMENT_VERSION,
                profiles: Vec::new(),
                extra: BTreeMap::new(),
            },
            Err(error) => return Err(SshProfileStoreError::Io(error.to_string())),
        };
        Ok(Self {
            path,
            document: Mutex::new(document),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn list(&self) -> Vec<SshProfile> {
        let mut profiles = self
            .document
            .lock()
            .expect("SSH profile store lock poisoned")
            .profiles
            .clone();
        profiles.sort_by_key(|profile| profile.name.to_lowercase());
        profiles
    }

    #[must_use]
    pub fn profile(&self, id: Uuid) -> Option<SshProfile> {
        self.document
            .lock()
            .expect("SSH profile store lock poisoned")
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
    }

    pub fn create(
        &self,
        name: impl Into<String>,
        host_alias: impl Into<String>,
        remote_command: Option<String>,
    ) -> Result<SshProfile, SshProfileStoreError> {
        let now = utc_timestamp();
        self.create_at(name, host_alias, remote_command, &now)
    }

    pub fn create_at(
        &self,
        name: impl Into<String>,
        host_alias: impl Into<String>,
        remote_command: Option<String>,
        timestamp: &str,
    ) -> Result<SshProfile, SshProfileStoreError> {
        let mut document = self
            .document
            .lock()
            .expect("SSH profile store lock poisoned");
        let profile = SshProfile {
            id: Uuid::new_v4(),
            name: name.into(),
            host_alias: host_alias.into(),
            remote_command,
            reviewed_fingerprint: None,
            created_at: timestamp.to_owned(),
            updated_at: timestamp.to_owned(),
            extra: BTreeMap::new(),
        };
        if document.profiles.len() >= SSH_PROFILE_MAX_COUNT {
            return Err(SshProfileStoreError::InvalidProfile);
        }
        if document
            .profiles
            .iter()
            .any(|candidate| candidate.name.eq_ignore_ascii_case(&profile.name))
        {
            return Err(SshProfileStoreError::DuplicateName);
        }
        let mut candidate = document.profiles.clone();
        candidate.push(profile.clone());
        let validated = validate_ssh_profiles(SshProfileDocument {
            version: document.version,
            profiles: candidate,
            extra: document.extra.clone(),
        })
        .map_err(|error| match error {
            SshCodecError::Invalid(_) => SshProfileStoreError::InvalidProfile,
            other => SshProfileStoreError::Codec(other),
        })?;
        self.save_locked(&validated)?;
        *document = validated;
        Ok(document
            .profiles
            .iter()
            .find(|candidate| candidate.id == profile.id)
            .expect("created SSH profile remains in the validated document")
            .clone())
    }

    pub fn update(
        &self,
        id: Uuid,
        name: Option<String>,
        host_alias: Option<String>,
        remote_command: Option<Option<String>>,
    ) -> Result<SshProfile, SshProfileStoreError> {
        let now = utc_timestamp();
        let mut document = self
            .document
            .lock()
            .expect("SSH profile store lock poisoned");
        let index = document
            .profiles
            .iter()
            .position(|profile| profile.id == id)
            .ok_or(SshProfileStoreError::MissingProfile)?;
        let mut candidate = document.clone();
        let updated_name = {
            let profile = &mut candidate.profiles[index];
            let previous_host = profile.host_alias.clone();
            let previous_command = profile.remote_command.clone();
            if let Some(name) = name {
                profile.name = name;
            }
            if let Some(host_alias) = host_alias {
                profile.host_alias = host_alias;
            }
            if let Some(remote_command) = remote_command {
                profile.remote_command = remote_command;
            }
            if profile.host_alias != previous_host || profile.remote_command != previous_command {
                profile.reviewed_fingerprint = None;
            }
            profile.updated_at = std::cmp::max(now, profile.created_at.clone());
            profile.name.clone()
        };
        if candidate
            .profiles
            .iter()
            .enumerate()
            .any(|(other, candidate)| {
                other != index && candidate.name.eq_ignore_ascii_case(&updated_name)
            })
        {
            return Err(SshProfileStoreError::DuplicateName);
        }
        let validated = validate_ssh_profiles(candidate).map_err(|error| match error {
            SshCodecError::Invalid(_) => SshProfileStoreError::InvalidProfile,
            other => SshProfileStoreError::Codec(other),
        })?;
        self.save_locked(&validated)?;
        *document = validated;
        let result = document.profiles[index].clone();
        Ok(result)
    }

    pub fn approve(&self, id: Uuid, fingerprint: &str) -> Result<SshProfile, SshProfileStoreError> {
        if fingerprint.len() != 64
            || !fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(SshProfileStoreError::InvalidProfile);
        }
        let mut document = self
            .document
            .lock()
            .expect("SSH profile store lock poisoned");
        let index = document
            .profiles
            .iter()
            .position(|profile| profile.id == id)
            .ok_or(SshProfileStoreError::MissingProfile)?;
        let mut candidate = document.clone();
        let profile = &mut candidate.profiles[index];
        profile.reviewed_fingerprint = Some(fingerprint.to_owned());
        profile.updated_at = std::cmp::max(utc_timestamp(), profile.created_at.clone());
        let result = profile.clone();
        let candidate = validate_ssh_profiles(candidate).map_err(|error| match error {
            SshCodecError::Invalid(_) => SshProfileStoreError::InvalidProfile,
            other => SshProfileStoreError::Codec(other),
        })?;
        self.save_locked(&candidate)?;
        *document = candidate;
        Ok(result)
    }

    pub fn delete(&self, id: Uuid) -> Result<(), SshProfileStoreError> {
        let mut document = self
            .document
            .lock()
            .expect("SSH profile store lock poisoned");
        let index = document
            .profiles
            .iter()
            .position(|profile| profile.id == id)
            .ok_or(SshProfileStoreError::MissingProfile)?;
        let mut candidate = document.clone();
        candidate.profiles.remove(index);
        let candidate = validate_ssh_profiles(candidate).map_err(|error| match error {
            SshCodecError::Invalid(_) => SshProfileStoreError::InvalidProfile,
            other => SshProfileStoreError::Codec(other),
        })?;
        self.save_locked(&candidate)?;
        *document = candidate;
        Ok(())
    }

    fn save_locked(&self, document: &SshProfileDocument) -> Result<(), SshProfileStoreError> {
        let data = encode_ssh_profiles(document.clone()).map_err(SshProfileStoreError::Codec)?;
        atomic_write_private(&self.path, &data)
            .map_err(|error| SshProfileStoreError::Io(error.to_string()))
    }
}

fn quarantine_profile_file(path: &Path) -> Result<(), SshProfileStoreError> {
    let parent = path.parent().ok_or(SshProfileStoreError::InvalidPath)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let destination = parent.join(format!(
        "{}.corrupt-{stamp}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or(SshProfileStoreError::InvalidPath)?,
        Uuid::new_v4()
    ));
    fs::rename(path, destination).map_err(|error| SshProfileStoreError::Io(error.to_string()))
}

fn utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs()) as libc::time_t;
    let mut broken_down = unsafe { std::mem::zeroed::<libc::tm>() };
    if unsafe { libc::gmtime_r(&seconds, &mut broken_down) }.is_null() {
        return "1970-01-01T00:00:00Z".to_owned();
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        broken_down.tm_year + 1900,
        broken_down.tm_mon + 1,
        broken_down.tm_mday,
        broken_down.tm_hour,
        broken_down.tm_min,
        broken_down.tm_sec
    )
}
