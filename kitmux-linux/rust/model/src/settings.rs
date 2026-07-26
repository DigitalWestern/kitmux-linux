use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;

pub const SETTINGS_VERSION: i64 = 1;
pub const SETTINGS_MAX_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RestoreLayoutPolicy {
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserSearchEngine {
    Google,
    Bing,
    #[serde(rename = "duckduckgo")]
    DuckDuckGo,
    Kagi,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceColorStyle {
    Stripe,
    Dot,
    Fill,
    Off,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceActiveStyle {
    Outline,
    Fill,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedSettings {
    pub restore_scrollback: bool,
    pub restore_layout: RestoreLayoutPolicy,
    pub sidebar_visible_on_launch: bool,
    pub paste_confirmation_threshold_bytes: u64,
    pub tab_title_shows_cwd: bool,
    pub tab_title_shows_process: bool,
    pub show_tab_close_button_on_hover: bool,
    pub new_pane_inherits_cwd: bool,
    pub notify_on_bell: bool,
    pub notify_on_terminal_notification: bool,
    pub confirm_close_with_running_process: bool,
    pub browser_search_engine: BrowserSearchEngine,
    pub workspace_color_style: WorkspaceColorStyle,
    pub workspace_active_style: WorkspaceActiveStyle,
    pub sidebar_width_points: u64,
    pub tab_min_width_points: u64,
    pub tab_max_width_points: u64,
}

impl Default for ValidatedSettings {
    fn default() -> Self {
        Self {
            restore_scrollback: false,
            restore_layout: RestoreLayoutPolicy::Always,
            sidebar_visible_on_launch: true,
            paste_confirmation_threshold_bytes: 8192,
            tab_title_shows_cwd: true,
            tab_title_shows_process: true,
            show_tab_close_button_on_hover: true,
            new_pane_inherits_cwd: false,
            notify_on_bell: false,
            notify_on_terminal_notification: true,
            confirm_close_with_running_process: false,
            browser_search_engine: BrowserSearchEngine::Google,
            workspace_color_style: WorkspaceColorStyle::Stripe,
            workspace_active_style: WorkspaceActiveStyle::Outline,
            sidebar_width_points: 180,
            tab_min_width_points: 90,
            tab_max_width_points: 220,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsDocument {
    raw: Map<String, Value>,
    resolved: ValidatedSettings,
}

impl SettingsDocument {
    #[must_use]
    pub fn raw(&self) -> &Map<String, Value> {
        &self.raw
    }

    #[must_use]
    pub const fn resolved(&self) -> &ValidatedSettings {
        &self.resolved
    }

    #[must_use]
    pub fn validated_values(&self) -> Map<String, Value> {
        validated_map(&self.raw)
    }

    pub fn replace_resolved(&mut self, resolved: ValidatedSettings) {
        self.resolved = resolved;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsCodecError {
    TooLarge,
    Malformed,
    UnsupportedVersion(i64),
}

impl fmt::Display for SettingsCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("settings exceed 1 MiB"),
            Self::Malformed => f.write_str("settings are not a JSON object"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported settings version {version}")
            }
        }
    }
}

impl std::error::Error for SettingsCodecError {}

pub fn decode_settings(data: &[u8]) -> Result<SettingsDocument, SettingsCodecError> {
    if data.len() > SETTINGS_MAX_BYTES {
        return Err(SettingsCodecError::TooLarge);
    }
    let raw: Map<String, Value> =
        serde_json::from_slice(data).map_err(|_| SettingsCodecError::Malformed)?;
    if let Some(version) = raw.get("version").and_then(Value::as_i64)
        && version > SETTINGS_VERSION
    {
        return Err(SettingsCodecError::UnsupportedVersion(version));
    }
    let resolved = resolve(&validated_map(&raw));
    Ok(SettingsDocument { raw, resolved })
}

pub fn encode_settings(document: &SettingsDocument) -> Result<Vec<u8>, SettingsCodecError> {
    let mut raw = document.raw.clone();
    let known = serde_json::to_value(&document.resolved)
        .map_err(|_| SettingsCodecError::Malformed)?
        .as_object()
        .cloned()
        .ok_or(SettingsCodecError::Malformed)?;
    for (key, value) in known {
        raw.insert(key, value);
    }
    raw.insert("version".to_owned(), Value::from(SETTINGS_VERSION));
    let bytes = serde_json::to_vec_pretty(&raw).map_err(|_| SettingsCodecError::Malformed)?;
    if bytes.len() > SETTINGS_MAX_BYTES {
        return Err(SettingsCodecError::TooLarge);
    }
    Ok(bytes)
}

fn validated_map(raw: &Map<String, Value>) -> Map<String, Value> {
    let mut valid = Map::new();
    accept_bool(raw, &mut valid, "restoreScrollback");
    accept_choice(raw, &mut valid, "restoreLayout", &["always", "never"]);
    accept_bool(raw, &mut valid, "sidebarVisibleOnLaunch");
    accept_integer(
        raw,
        &mut valid,
        "pasteConfirmationThresholdBytes",
        0,
        10_485_760,
    );
    accept_bool(raw, &mut valid, "tabTitleShowsCwd");
    accept_bool(raw, &mut valid, "tabTitleShowsProcess");
    accept_bool(raw, &mut valid, "showTabCloseButtonOnHover");
    accept_bool(raw, &mut valid, "newPaneInheritsCwd");
    accept_bool(raw, &mut valid, "notifyOnBell");
    accept_bool(raw, &mut valid, "notifyOnTerminalNotification");
    accept_bool(raw, &mut valid, "confirmCloseWithRunningProcess");
    accept_choice(
        raw,
        &mut valid,
        "browserSearchEngine",
        &["google", "bing", "duckduckgo", "kagi"],
    );
    accept_choice(
        raw,
        &mut valid,
        "workspaceColorStyle",
        &["stripe", "dot", "fill", "off"],
    );
    accept_choice(
        raw,
        &mut valid,
        "workspaceActiveStyle",
        &["outline", "fill"],
    );
    accept_integer(raw, &mut valid, "sidebarWidthPoints", 140, 320);
    accept_integer(raw, &mut valid, "tabMinWidthPoints", 60, 200);
    accept_integer(raw, &mut valid, "tabMaxWidthPoints", 120, 400);
    valid
}

fn resolve(valid: &Map<String, Value>) -> ValidatedSettings {
    let mut resolved = ValidatedSettings::default();
    macro_rules! bool_value {
        ($field:ident, $key:literal) => {
            if let Some(value) = valid.get($key).and_then(Value::as_bool) {
                resolved.$field = value;
            }
        };
    }
    bool_value!(restore_scrollback, "restoreScrollback");
    bool_value!(sidebar_visible_on_launch, "sidebarVisibleOnLaunch");
    bool_value!(tab_title_shows_cwd, "tabTitleShowsCwd");
    bool_value!(tab_title_shows_process, "tabTitleShowsProcess");
    bool_value!(show_tab_close_button_on_hover, "showTabCloseButtonOnHover");
    bool_value!(new_pane_inherits_cwd, "newPaneInheritsCwd");
    bool_value!(notify_on_bell, "notifyOnBell");
    bool_value!(
        notify_on_terminal_notification,
        "notifyOnTerminalNotification"
    );
    bool_value!(
        confirm_close_with_running_process,
        "confirmCloseWithRunningProcess"
    );
    resolved.restore_layout =
        decode_known(valid, "restoreLayout").unwrap_or(resolved.restore_layout);
    resolved.browser_search_engine =
        decode_known(valid, "browserSearchEngine").unwrap_or(resolved.browser_search_engine);
    resolved.workspace_color_style =
        decode_known(valid, "workspaceColorStyle").unwrap_or(resolved.workspace_color_style);
    resolved.workspace_active_style =
        decode_known(valid, "workspaceActiveStyle").unwrap_or(resolved.workspace_active_style);
    resolved.paste_confirmation_threshold_bytes = integer(valid, "pasteConfirmationThresholdBytes")
        .unwrap_or(resolved.paste_confirmation_threshold_bytes);
    resolved.sidebar_width_points =
        integer(valid, "sidebarWidthPoints").unwrap_or(resolved.sidebar_width_points);
    let minimum = integer(valid, "tabMinWidthPoints").unwrap_or(resolved.tab_min_width_points);
    let maximum = integer(valid, "tabMaxWidthPoints").unwrap_or(resolved.tab_max_width_points);
    if minimum <= maximum {
        resolved.tab_min_width_points = minimum;
        resolved.tab_max_width_points = maximum;
    }
    resolved
}

fn decode_known<T: for<'de> Deserialize<'de>>(raw: &Map<String, Value>, key: &str) -> Option<T> {
    serde_json::from_value(raw.get(key)?.clone()).ok()
}

fn integer(raw: &Map<String, Value>, key: &str) -> Option<u64> {
    raw.get(key)?.as_u64()
}

fn accept_bool(raw: &Map<String, Value>, valid: &mut Map<String, Value>, key: &str) {
    if raw.get(key).is_some_and(Value::is_boolean) {
        valid.insert(key.to_owned(), raw[key].clone());
    }
}

fn accept_choice(
    raw: &Map<String, Value>,
    valid: &mut Map<String, Value>,
    key: &str,
    allowed: &[&str],
) {
    if raw
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|value| allowed.contains(&value))
    {
        valid.insert(key.to_owned(), raw[key].clone());
    }
}

fn accept_integer(
    raw: &Map<String, Value>,
    valid: &mut Map<String, Value>,
    key: &str,
    minimum: u64,
    maximum: u64,
) {
    if raw
        .get(key)
        .and_then(Value::as_u64)
        .is_some_and(|value| (minimum..=maximum).contains(&value))
    {
        valid.insert(key.to_owned(), raw[key].clone());
    }
}
