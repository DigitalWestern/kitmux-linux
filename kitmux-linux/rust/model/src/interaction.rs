use crate::{CommandId, PixelRect, PixelSize, SettingsDocument};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasteConfirmationReason {
    Large { bytes: usize },
    ControlCharacters,
}

#[must_use]
pub fn paste_confirmation_reason(
    text: &str,
    large_byte_threshold: usize,
) -> Option<PasteConfirmationReason> {
    if text.len() > large_byte_threshold {
        return Some(PasteConfirmationReason::Large { bytes: text.len() });
    }
    text.chars()
        .any(|character| {
            let value = character as u32;
            value != 0x09 && value != 0x0a && value != 0x0d && (value < 0x20 || value == 0x7f)
        })
        .then_some(PasteConfirmationReason::ControlCharacters)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalCellCoordinate {
    pub column: u32,
    pub row: u32,
    pub in_left_half: bool,
    pub pixel_x: i32,
    pub pixel_y: i32,
}

#[must_use]
pub fn terminal_grid_size(
    frame: PixelRect,
    cell_width: i32,
    cell_height: i32,
) -> Option<PixelSize> {
    (frame.width > 0
        && frame.height > 0
        && cell_width > 0
        && cell_height > 0
        && frame.width / cell_width >= 2
        && frame.height / cell_height >= 1)
        .then(|| PixelSize::new(frame.width / cell_width, frame.height / cell_height))
}

#[must_use]
pub fn terminal_cell(
    x: f64,
    y: f64,
    frame: PixelRect,
    cell_width: i32,
    cell_height: i32,
) -> Option<TerminalCellCoordinate> {
    let grid = terminal_grid_size(frame, cell_width, cell_height)?;
    let grid_width = grid.width * cell_width;
    let grid_height = grid.height * cell_height;
    let pixel_x =
        ((x - f64::from(frame.x)).floor() as i64).clamp(0, i64::from(grid_width - 1)) as i32;
    let pixel_y =
        ((y - f64::from(frame.y)).floor() as i64).clamp(0, i64::from(grid_height - 1)) as i32;
    let column = pixel_x / cell_width;
    let row = pixel_y / cell_height;
    Some(TerminalCellCoordinate {
        column: column as u32,
        row: row as u32,
        in_left_half: (pixel_x - column * cell_width) * 2 < cell_width,
        pixel_x,
        pixel_y,
    })
}

#[must_use]
pub fn terminal_cell_scaled(
    logical_x: f64,
    logical_y: f64,
    backing_scale: f64,
    frame: PixelRect,
    cell_width: i32,
    cell_height: i32,
) -> Option<TerminalCellCoordinate> {
    (backing_scale.is_finite() && backing_scale > 0.0).then_some(())?;
    terminal_cell(
        logical_x * backing_scale,
        logical_y * backing_scale,
        frame,
        cell_width,
        cell_height,
    )
}

#[must_use]
pub fn accumulate_scroll_lines(delta_points: f64, cell_points: f64, residue: &mut f64) -> i32 {
    if !delta_points.is_finite() || !cell_points.is_finite() || cell_points <= 0.0 {
        return 0;
    }
    *residue += delta_points;
    let lines = (*residue / cell_points).trunc();
    let lines = lines.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    *residue -= f64::from(lines) * cell_points;
    lines
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ShortcutChord {
    pub key: char,
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShortcutAction {
    Copy,
    Paste,
    Search,
    CommandPalette,
    FontLarger,
    FontSmaller,
    FontReset,
    ClearScrollback,
    Navigation(CommandId),
    Select(NavigationTarget),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NavigationTarget {
    Workspace(usize),
    TerminalTab(usize),
}

pub struct ShortcutMap(HashMap<ShortcutChord, Option<ShortcutAction>>);

impl ShortcutMap {
    fn linux_default_bindings() -> Vec<(ShortcutChord, ShortcutAction)> {
        let chord = |key, shift| ShortcutChord {
            key,
            control: true,
            shift,
            alt: false,
            super_key: false,
        };
        let modified = |key, control, shift, alt, super_key| ShortcutChord {
            key,
            control,
            shift,
            alt,
            super_key,
        };
        vec![
            (chord('c', true), ShortcutAction::Copy),
            (chord('v', true), ShortcutAction::Paste),
            (chord('f', true), ShortcutAction::Search),
            (chord('p', true), ShortcutAction::CommandPalette),
            (chord('+', false), ShortcutAction::FontLarger),
            (chord('-', false), ShortcutAction::FontSmaller),
            (chord('0', false), ShortcutAction::FontReset),
            (chord('l', true), ShortcutAction::ClearScrollback),
            (
                modified('n', false, false, false, true),
                ShortcutAction::Navigation(CommandId::WorkspaceNew),
            ),
            (
                modified('t', false, false, false, true),
                ShortcutAction::Navigation(CommandId::TerminalNewTab),
            ),
            (
                modified('t', false, false, true, true),
                ShortcutAction::Navigation(CommandId::GroupNew),
            ),
            (
                modified('w', false, false, false, true),
                ShortcutAction::Navigation(CommandId::PaneClose),
            ),
            (
                modified('d', false, false, false, true),
                ShortcutAction::Navigation(CommandId::PaneSplitRight),
            ),
            (
                modified('d', false, true, false, true),
                ShortcutAction::Navigation(CommandId::PaneSplitDown),
            ),
            (
                modified('p', false, false, false, true),
                ShortcutAction::Navigation(CommandId::PaneFocusNext),
            ),
            (
                modified('p', false, true, false, true),
                ShortcutAction::Navigation(CommandId::PaneFocusPrevious),
            ),
            (
                modified('h', false, false, false, true),
                ShortcutAction::Navigation(CommandId::PaneFocusLeft),
            ),
            (
                modified('l', false, false, false, true),
                ShortcutAction::Navigation(CommandId::PaneFocusRight),
            ),
            (
                modified('k', false, false, false, true),
                ShortcutAction::Navigation(CommandId::PaneFocusUp),
            ),
            (
                modified('j', false, false, false, true),
                ShortcutAction::Navigation(CommandId::PaneFocusDown),
            ),
            (
                modified('h', false, true, false, true),
                ShortcutAction::Navigation(CommandId::PaneResizeLeft),
            ),
            (
                modified('l', false, true, false, true),
                ShortcutAction::Navigation(CommandId::PaneResizeRight),
            ),
            (
                modified('k', false, true, false, true),
                ShortcutAction::Navigation(CommandId::PaneResizeUp),
            ),
            (
                modified('j', false, true, false, true),
                ShortcutAction::Navigation(CommandId::PaneResizeDown),
            ),
            (
                modified('[', false, false, true, false),
                ShortcutAction::Navigation(CommandId::TerminalPreviousTab),
            ),
            (
                modified(']', false, false, true, false),
                ShortcutAction::Navigation(CommandId::TerminalNextTab),
            ),
            (
                modified('[', false, true, false, true),
                ShortcutAction::Navigation(CommandId::GroupPrevious),
            ),
            (
                modified(']', false, true, false, true),
                ShortcutAction::Navigation(CommandId::GroupNext),
            ),
        ]
    }

    #[must_use]
    pub fn linux_defaults() -> Self {
        Self::from_bindings(Self::linux_default_bindings())
    }

    #[must_use]
    pub fn linux_from_settings(settings: &SettingsDocument) -> Self {
        let mut bindings = Self::linux_default_bindings();
        let Some(overrides) = settings
            .raw()
            .get("linuxShortcutBindings")
            .and_then(serde_json::Value::as_object)
        else {
            return Self::from_bindings(bindings);
        };
        for (command, value) in overrides {
            let Some(action) = command.parse().ok().and_then(shortcut_action) else {
                continue;
            };
            let Some(chord) = value.as_object().and_then(shortcut_chord) else {
                continue;
            };
            if let Some(binding) = bindings
                .iter_mut()
                .find(|(_, current_action)| *current_action == action)
            {
                *binding = (chord, action);
            }
        }
        Self::from_bindings(bindings)
    }

    #[must_use]
    pub fn from_bindings(
        bindings: impl IntoIterator<Item = (ShortcutChord, ShortcutAction)>,
    ) -> Self {
        let mut resolved = HashMap::new();
        for (chord, action) in bindings {
            resolved
                .entry(chord)
                .and_modify(|value| *value = None)
                .or_insert(Some(action));
        }
        Self(resolved)
    }

    #[must_use]
    pub fn resolve(&self, chord: ShortcutChord) -> Option<ShortcutAction> {
        self.0.get(&chord).copied().flatten()
    }
}

fn shortcut_action(command: CommandId) -> Option<ShortcutAction> {
    match command {
        CommandId::TerminalCopy => Some(ShortcutAction::Copy),
        CommandId::TerminalPaste => Some(ShortcutAction::Paste),
        CommandId::TerminalFind => Some(ShortcutAction::Search),
        CommandId::TerminalClearScrollback => Some(ShortcutAction::ClearScrollback),
        CommandId::FontIncrease => Some(ShortcutAction::FontLarger),
        CommandId::FontDecrease => Some(ShortcutAction::FontSmaller),
        CommandId::FontReset => Some(ShortcutAction::FontReset),
        CommandId::WorkspaceNew
        | CommandId::GroupNew
        | CommandId::TerminalNewTab
        | CommandId::TerminalNextTab
        | CommandId::TerminalPreviousTab
        | CommandId::GroupNext
        | CommandId::GroupPrevious
        | CommandId::PaneClose
        | CommandId::GroupClose
        | CommandId::WorkspaceClose
        | CommandId::WorkspaceRename
        | CommandId::TerminalRenameTab
        | CommandId::GroupRename
        | CommandId::PaneSplitRight
        | CommandId::PaneSplitDown
        | CommandId::PaneFocusNext
        | CommandId::PaneFocusPrevious
        | CommandId::PaneFocusLeft
        | CommandId::PaneFocusRight
        | CommandId::PaneFocusUp
        | CommandId::PaneFocusDown
        | CommandId::PaneResizeLeft
        | CommandId::PaneResizeRight
        | CommandId::PaneResizeUp
        | CommandId::PaneResizeDown => Some(ShortcutAction::Navigation(command)),
        _ => None,
    }
}

#[must_use]
pub fn namespaced_number_target(chord: ShortcutChord) -> Option<NavigationTarget> {
    let index = chord.key.to_digit(10)? as usize;
    if !(1..=9).contains(&index) || chord.control || chord.shift {
        return None;
    }
    match (chord.alt, chord.super_key) {
        (false, true) => Some(NavigationTarget::Workspace(index - 1)),
        (true, false) => Some(NavigationTarget::TerminalTab(index - 1)),
        _ => None,
    }
}

fn shortcut_chord(raw: &serde_json::Map<String, serde_json::Value>) -> Option<ShortcutChord> {
    let key = raw.get("key")?.as_str()?;
    let mut characters = key.chars();
    let mut key = characters.next()?.to_ascii_lowercase();
    if characters.next().is_some() || key.is_control() || key.is_whitespace() {
        return None;
    }
    let modifier = |name| match raw.get(name) {
        None => Some(false),
        Some(value) => value.as_bool(),
    };
    let control = modifier("control")?;
    let mut shift = modifier("shift")?;
    let alt = modifier("alt")?;
    let super_key = modifier("super")?;
    if !(alt || super_key || (control && shift)) {
        return None;
    }
    if (key == '=' || key == '+') && shift {
        key = '+';
        shift = false;
    }
    Some(ShortcutChord {
        key,
        control,
        shift,
        alt,
        super_key,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalUrlSegment {
    pub row: usize,
    pub columns: std::ops::Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalUrlMatch {
    pub url: String,
    pub segments: Vec<TerminalUrlSegment>,
}

#[must_use]
pub fn detected_url(
    rows: &[String],
    row: usize,
    column: usize,
    terminal_columns: usize,
    row_wraps: Option<&[bool]>,
) -> Option<TerminalUrlMatch> {
    const MAXIMUM_JOINED_ROWS: usize = 20;
    if terminal_columns == 0 || row >= rows.len() || column >= rows[row].len().min(terminal_columns)
    {
        return None;
    }
    let wraps = |index: usize| {
        row_wraps
            .and_then(|values| values.get(index))
            .copied()
            .unwrap_or(rows[index].len() >= terminal_columns)
    };
    let mut first = row;
    while first > 0 && row - first + 1 < MAXIMUM_JOINED_ROWS && wraps(first - 1) {
        first -= 1;
    }
    let mut last = row;
    while last + 1 < rows.len() && last - first + 1 < MAXIMUM_JOINED_ROWS && wraps(last) {
        last += 1;
    }
    let mut joined = String::new();
    let mut offsets = Vec::new();
    for (visual_row, row_text) in rows.iter().enumerate().take(last + 1).skip(first) {
        let part = &row_text.as_bytes()[..row_text.len().min(terminal_columns)];
        let start = joined.len();
        joined.push_str(std::str::from_utf8(part).ok()?);
        offsets.push((visual_row, start, part.len()));
    }
    let clicked = offsets.iter().find(|item| item.0 == row)?.1 + column;
    let (url, range) = link_at(&joined, clicked)?;
    let segments = offsets
        .into_iter()
        .filter_map(|(visual_row, start, length)| {
            let lower = range.start.max(start);
            let upper = range.end.min(start + length);
            (lower < upper).then_some(TerminalUrlSegment {
                row: visual_row,
                columns: lower - start..upper - start,
            })
        })
        .collect();
    Some(TerminalUrlMatch { url, segments })
}

fn link_at(text: &str, offset: usize) -> Option<(String, std::ops::Range<usize>)> {
    let mut cursor = 0;
    for raw in text.split_whitespace() {
        let raw_start = cursor + text[cursor..].find(raw)?;
        cursor = raw_start + raw.len();
        let token = raw.trim_start_matches(['(', '[', '{', '<', '"', '\'']);
        let token_start = raw_start + raw.len() - token.len();
        let token = token.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}']);
        let end = token_start + token.len();
        if !(token_start..end).contains(&offset) {
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if ["http://", "https://", "mailto:", "file://", "ftp://"]
            .iter()
            .any(|scheme| lower.starts_with(scheme))
        {
            return Some((token.to_owned(), token_start..end));
        }
        if token.contains('@')
            && token
                .split_once('@')
                .is_some_and(|(_, domain)| domain.contains('.'))
        {
            return Some((format!("mailto:{token}"), token_start..end));
        }
    }
    None
}
