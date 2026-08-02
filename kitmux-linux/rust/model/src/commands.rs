use crate::{Direction, SplitAxis};
use std::str::FromStr;

macro_rules! commands {
    ($(($variant:ident, $id:literal, $action:expr)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub enum CommandId { $($variant),+ }

        impl CommandId {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $id),+ }
            }

            #[must_use]
            pub const fn action(self) -> SemanticAction {
                match self { $(Self::$variant => $action),+ }
            }
        }

        impl FromStr for CommandId {
            type Err = ();
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                if value.is_empty() || value.len() > 128 || value.as_bytes().contains(&0) {
                    return Err(());
                }
                match value { $($id => Ok(Self::$variant),)+ _ => Err(()) }
            }
        }
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticAction {
    NewTerminalTab,
    NewBrowserPane,
    Split(SplitAxis),
    FindInTerminal,
    NewWorkspace,
    NewGroup,
    CycleTab(i8),
    CycleGroup(i8),
    ClosePane,
    CloseGroup,
    CloseWorkspace,
    RenameWorkspace,
    RenameTab,
    RenameGroup,
    CyclePaneFocus(i8),
    MovePaneFocus(Direction),
    ResizePane(Direction),
    Paste,
    Copy,
    ClearScrollback,
    JumpToUnread,
    SetResumeCommand,
    OpenSettings,
    InstallCommandLineTool,
    ReloadKittyConfig,
    FontDelta(i8),
    FontReset,
}

commands!(
    (
        TerminalNewTab,
        "terminal.new-tab",
        SemanticAction::NewTerminalTab
    ),
    (
        BrowserNewPane,
        "browser.new-pane",
        SemanticAction::NewBrowserPane
    ),
    (
        PaneSplitRight,
        "pane.split-right",
        SemanticAction::Split(SplitAxis::LeftRight)
    ),
    (
        PaneSplitDown,
        "pane.split-down",
        SemanticAction::Split(SplitAxis::TopBottom)
    ),
    (
        TerminalFind,
        "terminal.find",
        SemanticAction::FindInTerminal
    ),
    (WorkspaceNew, "workspace.new", SemanticAction::NewWorkspace),
    (GroupNew, "group.new", SemanticAction::NewGroup),
    (
        TerminalNextTab,
        "terminal.next-tab",
        SemanticAction::CycleTab(1)
    ),
    (
        TerminalPreviousTab,
        "terminal.previous-tab",
        SemanticAction::CycleTab(-1)
    ),
    (GroupNext, "group.next", SemanticAction::CycleGroup(1)),
    (
        GroupPrevious,
        "group.previous",
        SemanticAction::CycleGroup(-1)
    ),
    (PaneClose, "pane.close", SemanticAction::ClosePane),
    (GroupClose, "group.close", SemanticAction::CloseGroup),
    (
        WorkspaceClose,
        "workspace.close",
        SemanticAction::CloseWorkspace
    ),
    (
        WorkspaceRename,
        "workspace.rename",
        SemanticAction::RenameWorkspace
    ),
    (
        TerminalRenameTab,
        "terminal.rename-tab",
        SemanticAction::RenameTab
    ),
    (GroupRename, "group.rename", SemanticAction::RenameGroup),
    (
        PaneFocusNext,
        "pane.focus-next",
        SemanticAction::CyclePaneFocus(1)
    ),
    (
        PaneFocusPrevious,
        "pane.focus-previous",
        SemanticAction::CyclePaneFocus(-1)
    ),
    (
        PaneFocusLeft,
        "pane.focus-left",
        SemanticAction::MovePaneFocus(Direction::Left)
    ),
    (
        PaneFocusRight,
        "pane.focus-right",
        SemanticAction::MovePaneFocus(Direction::Right)
    ),
    (
        PaneFocusUp,
        "pane.focus-up",
        SemanticAction::MovePaneFocus(Direction::Up)
    ),
    (
        PaneFocusDown,
        "pane.focus-down",
        SemanticAction::MovePaneFocus(Direction::Down)
    ),
    (
        PaneResizeLeft,
        "pane.resize-left",
        SemanticAction::ResizePane(Direction::Left)
    ),
    (
        PaneResizeRight,
        "pane.resize-right",
        SemanticAction::ResizePane(Direction::Right)
    ),
    (
        PaneResizeUp,
        "pane.resize-up",
        SemanticAction::ResizePane(Direction::Up)
    ),
    (
        PaneResizeDown,
        "pane.resize-down",
        SemanticAction::ResizePane(Direction::Down)
    ),
    (TerminalPaste, "terminal.paste", SemanticAction::Paste),
    (TerminalCopy, "terminal.copy", SemanticAction::Copy),
    (
        TerminalClearScrollback,
        "terminal.clear-scrollback",
        SemanticAction::ClearScrollback
    ),
    (
        NotificationJumpUnread,
        "notification.jump-unread",
        SemanticAction::JumpToUnread
    ),
    (
        TerminalResumeCommand,
        "terminal.resume-command",
        SemanticAction::SetResumeCommand
    ),
    (AppSettings, "app.settings", SemanticAction::OpenSettings),
    (
        AppInstallCommandLineTool,
        "app.install-command-line-tool",
        SemanticAction::InstallCommandLineTool
    ),
    (
        AppReloadKittyConfig,
        "app.reload-kitty-config",
        SemanticAction::ReloadKittyConfig
    ),
    (FontIncrease, "font.increase", SemanticAction::FontDelta(2)),
    (FontDecrease, "font.decrease", SemanticAction::FontDelta(-2)),
    (FontReset, "font.reset", SemanticAction::FontReset),
);

#[must_use]
pub fn command_palette_matches(query: &str) -> Vec<CommandId> {
    let query = query.trim().to_ascii_lowercase().replace(' ', "-");
    let mut matches = CommandId::ALL
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, command)| {
            let id = command.as_str();
            (query.is_empty() || id.contains(&query)).then_some((
                if id == query {
                    0
                } else if id.starts_with(&query) {
                    1
                } else if id.split(['.', '-']).any(|part| part.starts_with(&query)) {
                    2
                } else {
                    3
                },
                index,
                command,
            ))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|(score, index, _)| (*score, *index));
    matches.into_iter().map(|(_, _, command)| command).collect()
}
