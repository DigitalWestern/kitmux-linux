//! Display-free Kitmux product model.
//!
//! This crate owns identity, hierarchy, split geometry, navigation, close
//! cascading, bounded persistence/control contracts, Linux path/file adapters,
//! and the lifetime of abstract pane runtimes. It deliberately has no GTK,
//! WebKit, libkitty, shell-execution, or network-runtime dependency.

mod commands;
mod control;
mod ids;
mod import;
mod interaction;
mod model;
mod persistence;
mod platform;
mod runtime;
mod settings;
mod split;
mod ssh;
mod state;

pub use commands::{CommandId, SemanticAction, command_palette_matches};
pub use control::{
    CONTROL_MAX_REQUEST_BYTES, CONTROL_MAX_RESPONSE_BYTES, ControlCodecError, ControlError,
    ControlMethod, ControlRequest, ControlResponse, LineFrameDecoder, decode_control_request,
    decode_control_response, encode_control_request, encode_control_response,
};
pub use ids::{GroupId, PaneId, SplitId, SurfaceId, TabId, WorkspaceId};
pub use import::{
    ImportPreviewError, ImportPreviewNote, ImportPreviewTranslation, InertImportCommand,
    MacosStateImportPreview, preview_macos_state, preview_macos_state_file,
};
pub use interaction::{
    NavigationTarget, PasteConfirmationReason, ShortcutAction, ShortcutChord, ShortcutMap,
    TerminalCellCoordinate, TerminalUrlMatch, TerminalUrlSegment, accumulate_scroll_lines,
    detected_url, namespaced_number_target, paste_confirmation_reason, terminal_cell,
    terminal_cell_scaled, terminal_grid_size,
};
pub use model::{
    AppModel, CloseOutcome, CloseScope, GroupModel, ModelError, RuntimeLocation,
    RuntimePresentation, TabModel, WorkspaceModel,
};
pub use persistence::{
    LoadDisposition, SettingsLoad, StateLoad, last_good_path, load_settings_at_launch,
    load_state_at_launch, reload_settings, save_settings, save_state,
};
pub use platform::{
    AtomicWriteError, FileChange, FileFingerprint, PollingFileWatcher, RuntimePathError,
    UnixSocketAddress, XdgPaths, atomic_write_private, read_bounded, sha256_bytes, sha256_file,
};
pub use runtime::{
    BrowserRuntime, MockBrowserRuntime, MockRuntimeProbe, MockRuntimeSnapshot, MockTerminalRuntime,
    PaneContainer, PaneContainerError, PaneRuntime, PaneSurface, RuntimeKind, TerminalRuntime,
};
pub use settings::{
    BrowserSearchEngine, RestoreLayoutPolicy, SETTINGS_MAX_BYTES, SettingsCodecError,
    SettingsDocument, ValidatedSettings, WorkspaceActiveStyle, WorkspaceColorStyle,
    decode_settings, encode_settings,
};
pub use split::{
    Direction, PixelRect, PixelSize, ResizeTarget, Split, SplitAxis, SplitLayout, SplitNode,
    directional_neighbor,
};
pub use ssh::{
    SSH_DOCUMENT_MAX_BYTES, SSH_RESOLUTION_MAX_BYTES, SshCodecError, SshConnectionReview,
    SshForward, SshForwardKind, SshProfile, SshProfileDocument, SshResolution, decode_ssh_profiles,
    encode_ssh_profiles,
};
pub use state::{
    AppSnapshot, PaneContentKind, PaneDetail, PaneSurfaceDetail, SNAPSHOT_MAX_BYTES,
    SNAPSHOT_VERSION, SnapshotCodecError, TabGroupSnapshot, TerminalTabSnapshot, WorkspaceSnapshot,
    decode_snapshot, encode_snapshot, valid_resume_command,
};
