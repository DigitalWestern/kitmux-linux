use crate::{PaneId, SplitNode, WorkspaceId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use uuid::Uuid;

pub const SNAPSHOT_VERSION: i64 = 1;
pub const SNAPSHOT_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const RESUME_COMMAND_MAX_BYTES: usize = 2048;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub version: i64,
    pub active_workspace_index: i64,
    pub created_workspace_count: i64,
    pub workspaces: Vec<WorkspaceSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<WorkspaceId>,
    pub name: String,
    pub active_tab_group_index: i64,
    pub created_group_count: i64,
    pub tab_groups: Vec<TabGroupSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_index: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabGroupSnapshot {
    pub name: String,
    pub active_terminal_tab_index: i64,
    pub terminal_tabs: Vec<TerminalTabSnapshot>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTabSnapshot {
    #[serde(rename = "focusedPaneID")]
    pub focused_pane_id: PaneId,
    pub root: SplitNode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_details: Option<BTreeMap<String, PaneDetail>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneDetail {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<PaneContentKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "sshProfileID")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_profile_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surfaces: Option<Vec<PaneSurfaceDetail>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_surface_index: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneSurfaceDetail {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
    pub kind: PaneContentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneContentKind {
    Terminal,
    Browser,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotCodecError {
    TooLarge,
    Malformed,
    UnsupportedVersion(i64),
    Invalid(&'static str),
}

impl fmt::Display for SnapshotCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("snapshot exceeds 8 MiB"),
            Self::Malformed => f.write_str("snapshot is not valid JSON"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported snapshot version {version}")
            }
            Self::Invalid(reason) => write!(f, "invalid snapshot: {reason}"),
        }
    }
}

impl std::error::Error for SnapshotCodecError {}

#[must_use]
pub fn valid_resume_command(command: Option<&str>) -> Option<String> {
    let trimmed = command?.trim();
    (!trimmed.is_empty()
        && trimmed.len() <= RESUME_COMMAND_MAX_BYTES
        && !trimmed.chars().any(|character| character.is_control()))
    .then(|| trimmed.to_owned())
}

pub fn decode_snapshot(data: &[u8]) -> Result<AppSnapshot, SnapshotCodecError> {
    if data.len() > SNAPSHOT_MAX_BYTES {
        return Err(SnapshotCodecError::TooLarge);
    }
    let value: serde_json::Value =
        serde_json::from_slice(data).map_err(|_| SnapshotCodecError::Malformed)?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_i64)
        .ok_or(SnapshotCodecError::Malformed)?;
    if version > SNAPSHOT_VERSION {
        return Err(SnapshotCodecError::UnsupportedVersion(version));
    }
    let snapshot: AppSnapshot =
        serde_json::from_value(value).map_err(|_| SnapshotCodecError::Malformed)?;
    validate_snapshot(snapshot)
}

pub fn encode_snapshot(snapshot: AppSnapshot) -> Result<Vec<u8>, SnapshotCodecError> {
    let snapshot = validate_snapshot(snapshot)?;
    let bytes = serde_json::to_vec_pretty(&snapshot).map_err(|_| SnapshotCodecError::Malformed)?;
    if bytes.len() > SNAPSHOT_MAX_BYTES {
        return Err(SnapshotCodecError::TooLarge);
    }
    Ok(bytes)
}

fn validate_snapshot(mut snapshot: AppSnapshot) -> Result<AppSnapshot, SnapshotCodecError> {
    if snapshot.workspaces.is_empty() {
        return Err(SnapshotCodecError::Invalid("empty workspace list"));
    }
    snapshot.version = SNAPSHOT_VERSION;
    if snapshot
        .font_size
        .is_some_and(|size| !size.is_finite() || !(4.0..=512.0).contains(&size))
    {
        snapshot.font_size = None;
    }
    let mut workspaces = HashSet::new();
    let mut panes = HashSet::new();
    for workspace in &mut snapshot.workspaces {
        let id = workspace.id.unwrap_or_default();
        workspace.id = Some(id);
        if !workspaces.insert(id) {
            return Err(SnapshotCodecError::Invalid("duplicate workspace ID"));
        }
        if workspace.tab_groups.is_empty() {
            return Err(SnapshotCodecError::Invalid("empty tab-group list"));
        }
        for group in &mut workspace.tab_groups {
            if group.terminal_tabs.is_empty() {
                return Err(SnapshotCodecError::Invalid("empty terminal-tab list"));
            }
            for tab in &mut group.terminal_tabs {
                if !tab.root.has_unique_ids_and_valid_ratios() {
                    return Err(SnapshotCodecError::Invalid("invalid split tree"));
                }
                let pane_ids = tab.root.pane_ids();
                for pane_id in &pane_ids {
                    if !panes.insert(*pane_id) {
                        return Err(SnapshotCodecError::Invalid("duplicate pane ID"));
                    }
                }
                if !pane_ids.contains(&tab.focused_pane_id) {
                    tab.focused_pane_id = pane_ids[0];
                }
                tab.custom_title = tab
                    .custom_title
                    .take()
                    .map(|title| title.trim().to_owned())
                    .filter(|title| !title.is_empty());
                if let Some(details) = tab.pane_details.take() {
                    let valid_ids: HashSet<String> =
                        pane_ids.iter().map(ToString::to_string).collect();
                    let mut valid = BTreeMap::new();
                    for (raw_id, mut detail) in details {
                        if !valid_ids.contains(&raw_id) {
                            continue;
                        }
                        repair_detail(&mut detail);
                        if detail_has_data(&detail) {
                            valid.insert(raw_id, detail);
                        }
                    }
                    tab.pane_details = (!valid.is_empty()).then_some(valid);
                }
            }
            group.active_terminal_tab_index =
                clamp_index(group.active_terminal_tab_index, group.terminal_tabs.len());
        }
        workspace.active_tab_group_index =
            clamp_index(workspace.active_tab_group_index, workspace.tab_groups.len());
        workspace.created_group_count = workspace
            .created_group_count
            .max(workspace.tab_groups.len() as i64);
    }
    snapshot.active_workspace_index =
        clamp_index(snapshot.active_workspace_index, snapshot.workspaces.len());
    snapshot.created_workspace_count = snapshot
        .created_workspace_count
        .max(snapshot.workspaces.len() as i64);
    Ok(snapshot)
}

fn repair_detail(detail: &mut PaneDetail) {
    if let Some(mut stack) = detail.surfaces.take().filter(|stack| !stack.is_empty()) {
        let mut surface_ids = HashSet::new();
        stack.retain_mut(|surface| {
            if !surface_ids.insert(surface.id) {
                return false;
            }
            repair_surface(surface);
            true
        });
        if !stack.is_empty() {
            detail.active_surface_index = Some(clamp_index(
                detail.active_surface_index.unwrap_or(0),
                stack.len(),
            ));
            detail.surfaces = Some(stack);
            detail.cwd = None;
            detail.resume_command = None;
            detail.kind = None;
            detail.url = None;
            detail.ssh_profile_id = None;
            return;
        }
    }
    detail.surfaces = None;
    detail.active_surface_index = None;
    match detail.kind {
        Some(PaneContentKind::Browser) => {
            detail.cwd = None;
            detail.resume_command = None;
            detail.ssh_profile_id = None;
            detail.url = valid_url(detail.url.take());
        }
        _ => {
            detail.kind = None;
            detail.url = None;
            detail.cwd = valid_absolute_path(detail.cwd.take());
            detail.resume_command = valid_resume_command(detail.resume_command.as_deref());
            if detail.ssh_profile_id.is_some() {
                detail.cwd = None;
                detail.resume_command = None;
            }
        }
    }
}

fn repair_surface(surface: &mut PaneSurfaceDetail) {
    match surface.kind {
        PaneContentKind::Browser => {
            surface.cwd = None;
            surface.resume_command = None;
            surface.url = valid_url(surface.url.take());
        }
        PaneContentKind::Terminal => {
            surface.url = None;
            surface.cwd = valid_absolute_path(surface.cwd.take());
            surface.resume_command = valid_resume_command(surface.resume_command.as_deref());
        }
    }
}

fn valid_absolute_path(value: Option<String>) -> Option<String> {
    value.filter(|path| path.starts_with('/') && !path.chars().any(|c| c.is_control()))
}

fn valid_url(value: Option<String>) -> Option<String> {
    value.map(|url| url.trim().to_owned()).filter(|url| {
        let authority = url
            .split_once("://")
            .map(|(_, remainder)| remainder)
            .and_then(|remainder| remainder.split(['/', '?', '#']).next());
        url.len() <= 8192
            && !url.chars().any(|c| c.is_control())
            && !url.chars().any(char::is_whitespace)
            && (url.starts_with("http://") || url.starts_with("https://"))
            && authority.is_some_and(|host| !host.is_empty())
    })
}

fn detail_has_data(detail: &PaneDetail) -> bool {
    detail.surfaces.is_some()
        || detail.kind == Some(PaneContentKind::Browser)
        || detail.cwd.is_some()
        || detail.resume_command.is_some()
        || detail.ssh_profile_id.is_some()
}

fn clamp_index(index: i64, count: usize) -> i64 {
    index.clamp(0, count.saturating_sub(1) as i64)
}
