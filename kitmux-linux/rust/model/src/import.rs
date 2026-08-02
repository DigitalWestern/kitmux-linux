use crate::{
    SNAPSHOT_MAX_BYTES, SnapshotCodecError, decode_snapshot, read_bounded, sha256_bytes,
    valid_resume_command,
};
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewNote {
    pub field: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewTranslation {
    pub field: String,
    pub from: Value,
    pub to: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InertImportCommand {
    pub field: String,
    pub command: String,
    pub requires_explicit_approval: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacosStateImportPreview {
    pub source_sha256: String,
    pub accepted: Vec<ImportPreviewNote>,
    pub translated: Vec<ImportPreviewTranslation>,
    pub rejected: Vec<ImportPreviewNote>,
    pub inert_commands: Vec<InertImportCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportPreviewError {
    Io(ErrorKind),
    InvalidTargetHome,
    Malformed,
    TooLarge,
}

impl fmt::Display for ImportPreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(formatter, "could not read source state: {kind}"),
            Self::InvalidTargetHome => {
                formatter.write_str("Linux home must be an existing absolute directory")
            }
            Self::Malformed => formatter.write_str("source state is not valid JSON"),
            Self::TooLarge => formatter.write_str("source state exceeds 8 MiB"),
        }
    }
}

impl std::error::Error for ImportPreviewError {}

pub fn preview_macos_state_file(
    source: &Path,
    linux_home: &Path,
) -> Result<MacosStateImportPreview, ImportPreviewError> {
    let bytes = read_bounded(source, SNAPSHOT_MAX_BYTES as u64).map_err(|error| {
        if error.kind() == ErrorKind::FileTooLarge {
            ImportPreviewError::TooLarge
        } else {
            ImportPreviewError::Io(error.kind())
        }
    })?;
    preview_macos_state(&bytes, linux_home)
}

pub fn preview_macos_state(
    data: &[u8],
    linux_home: &Path,
) -> Result<MacosStateImportPreview, ImportPreviewError> {
    if data.len() > SNAPSHOT_MAX_BYTES {
        return Err(ImportPreviewError::TooLarge);
    }
    if !linux_home.is_absolute() || !linux_home.is_dir() {
        return Err(ImportPreviewError::InvalidTargetHome);
    }
    let raw: Value = serde_json::from_slice(data).map_err(|_| ImportPreviewError::Malformed)?;
    let mut preview = MacosStateImportPreview {
        source_sha256: sha256_bytes(data),
        accepted: Vec::new(),
        translated: Vec::new(),
        rejected: Vec::new(),
        inert_commands: Vec::new(),
    };

    match decode_snapshot(data) {
        Ok(snapshot) => {
            let (workspaces, groups, tabs, panes, surfaces) = snapshot_counts(&snapshot);
            preview.accepted.push(ImportPreviewNote {
                field: "/".to_owned(),
                detail: format!(
                    "{workspaces} workspaces, {groups} groups, {tabs} tabs, \
                     {panes} panes, {surfaces} explicit surfaces"
                ),
            });
            let repaired =
                serde_json::to_value(snapshot).map_err(|_| ImportPreviewError::Malformed)?;
            classify_value(&raw, Some(&repaired), "", linux_home, &mut preview);
        }
        Err(SnapshotCodecError::UnsupportedVersion(version)) => {
            preview.rejected.push(ImportPreviewNote {
                field: "/version".to_owned(),
                detail: format!(
                    "schema version {version} is newer than supported; source left untouched"
                ),
            });
        }
        Err(SnapshotCodecError::Invalid(reason)) => {
            preview.rejected.push(ImportPreviewNote {
                field: "/".to_owned(),
                detail: format!("snapshot rejected: {reason}; source left untouched"),
            });
        }
        Err(SnapshotCodecError::Malformed) => return Err(ImportPreviewError::Malformed),
        Err(SnapshotCodecError::TooLarge) => return Err(ImportPreviewError::TooLarge),
    }
    Ok(preview)
}

fn snapshot_counts(snapshot: &crate::AppSnapshot) -> (usize, usize, usize, usize, usize) {
    let mut groups = 0;
    let mut tabs = 0;
    let mut panes = 0;
    let mut surfaces = 0;
    for workspace in &snapshot.workspaces {
        groups += workspace.tab_groups.len();
        for group in &workspace.tab_groups {
            tabs += group.terminal_tabs.len();
            for tab in &group.terminal_tabs {
                panes += tab.root.pane_ids().len();
                surfaces += tab
                    .pane_details
                    .as_ref()
                    .into_iter()
                    .flat_map(|details| details.values())
                    .filter_map(|detail| detail.surfaces.as_ref())
                    .map(Vec::len)
                    .sum::<usize>();
            }
        }
    }
    (snapshot.workspaces.len(), groups, tabs, panes, surfaces)
}

fn classify_value(
    raw: &Value,
    repaired: Option<&Value>,
    field: &str,
    linux_home: &Path,
    preview: &mut MacosStateImportPreview,
) {
    let key = field.rsplit('/').next().unwrap_or_default();
    match key {
        "resumeCommand" => {
            classify_command(raw, field, preview);
            return;
        }
        "cwd" => {
            classify_path(raw, field, linux_home, preview);
            return;
        }
        "url" => {
            classify_url(raw, repaired, field, preview);
            return;
        }
        "sshProfileID" => {
            if !raw.is_null() {
                preview.rejected.push(ImportPreviewNote {
                    field: field.to_owned(),
                    detail: "macOS SSH profile references require a separate profile import"
                        .to_owned(),
                });
            }
            return;
        }
        _ => {}
    }

    match (raw, repaired) {
        (Value::Object(raw), Some(Value::Object(repaired))) => {
            for (key, value) in raw {
                classify_value(
                    value,
                    repaired.get(key),
                    &child_field(field, key),
                    linux_home,
                    preview,
                );
            }
            for (key, value) in repaired {
                if !raw.contains_key(key) {
                    preview.translated.push(ImportPreviewTranslation {
                        field: child_field(field, key),
                        from: Value::Null,
                        to: value.clone(),
                    });
                }
            }
        }
        (Value::Array(raw), Some(Value::Array(repaired))) => {
            for (index, value) in raw.iter().enumerate() {
                classify_value(
                    value,
                    repaired.get(index),
                    &child_field(field, &index.to_string()),
                    linux_home,
                    preview,
                );
            }
            if raw.len() != repaired.len() {
                preview.translated.push(ImportPreviewTranslation {
                    field: field.to_owned(),
                    from: Value::from(raw.len()),
                    to: Value::from(repaired.len()),
                });
            }
        }
        (_, Some(repaired)) if raw != repaired => {
            preview.translated.push(ImportPreviewTranslation {
                field: field.to_owned(),
                from: raw.clone(),
                to: repaired.clone(),
            });
        }
        (_, None) if !raw.is_null() => preview.rejected.push(ImportPreviewNote {
            field: field.to_owned(),
            detail: "field is unsupported or unsafe on Linux".to_owned(),
        }),
        _ => {}
    }
}

fn classify_command(raw: &Value, field: &str, preview: &mut MacosStateImportPreview) {
    let Some(command) = raw.as_str() else {
        if !raw.is_null() {
            preview.rejected.push(ImportPreviewNote {
                field: field.to_owned(),
                detail: "resume command is not text".to_owned(),
            });
        }
        return;
    };
    if let Some(command) = valid_resume_command(Some(command)) {
        preview.inert_commands.push(InertImportCommand {
            field: field.to_owned(),
            command,
            requires_explicit_approval: true,
        });
    } else {
        preview.rejected.push(ImportPreviewNote {
            field: field.to_owned(),
            detail: "resume command is empty, oversized, or contains controls".to_owned(),
        });
    }
}

fn classify_url(
    raw: &Value,
    repaired: Option<&Value>,
    field: &str,
    preview: &mut MacosStateImportPreview,
) {
    if raw.is_null() {
        return;
    }
    match repaired.and_then(Value::as_str) {
        Some(value) if raw.as_str() == Some(value) => preview.accepted.push(ImportPreviewNote {
            field: field.to_owned(),
            detail: "portable HTTP(S) URL".to_owned(),
        }),
        Some(value) => preview.translated.push(ImportPreviewTranslation {
            field: field.to_owned(),
            from: raw.clone(),
            to: Value::String(value.to_owned()),
        }),
        None => preview.rejected.push(ImportPreviewNote {
            field: field.to_owned(),
            detail: "URL is not a safe portable HTTP(S) value".to_owned(),
        }),
    }
}

fn classify_path(
    raw: &Value,
    field: &str,
    linux_home: &Path,
    preview: &mut MacosStateImportPreview,
) {
    let Some(value) = raw.as_str() else {
        if !raw.is_null() {
            preview.rejected.push(ImportPreviewNote {
                field: field.to_owned(),
                detail: "cwd is not text".to_owned(),
            });
        }
        return;
    };
    let path = Path::new(value);
    if !safe_absolute_path(path) {
        preview.rejected.push(ImportPreviewNote {
            field: field.to_owned(),
            detail: "cwd is not a safe absolute path".to_owned(),
        });
        return;
    }
    if let Some(relative) = macos_home_relative_path(path) {
        let translated = linux_home.join(relative);
        if translated.is_dir() {
            preview.translated.push(ImportPreviewTranslation {
                field: field.to_owned(),
                from: Value::String(value.to_owned()),
                to: Value::String(translated.to_string_lossy().into_owned()),
            });
        } else {
            preview.rejected.push(ImportPreviewNote {
                field: field.to_owned(),
                detail: "translated Linux cwd does not exist".to_owned(),
            });
        }
    } else if path.is_dir() {
        preview.accepted.push(ImportPreviewNote {
            field: field.to_owned(),
            detail: "existing Linux directory".to_owned(),
        });
    } else {
        preview.rejected.push(ImportPreviewNote {
            field: field.to_owned(),
            detail: "cwd does not exist on Linux".to_owned(),
        });
    }
}

fn child_field(parent: &str, child: &str) -> String {
    format!("{parent}/{}", child.replace('~', "~0").replace('/', "~1"))
}

fn safe_absolute_path(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

fn macos_home_relative_path(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    if components.next()? != Component::RootDir
        || components.next()? != Component::Normal("Users".as_ref())
    {
        return None;
    }
    match components.next()? {
        Component::Normal(_) => {}
        _ => return None,
    }
    Some(components.collect())
}
