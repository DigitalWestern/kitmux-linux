//! Display-free Kitmux product model.
//!
//! This crate owns identity, hierarchy, split geometry, navigation, close
//! cascading, bounded persistence/control contracts, Linux path/file adapters,
//! and the lifetime of abstract pane runtimes. It deliberately has no GTK,
//! WebKit, libkitty, shell-execution, or network-runtime dependency.

mod commands;
mod control;
mod ids;
mod model;
mod platform;
mod runtime;
mod settings;
mod split;
mod state;

pub use commands::{CommandId, SemanticAction};
pub use control::{
    CONTROL_MAX_REQUEST_BYTES, CONTROL_MAX_RESPONSE_BYTES, ControlCodecError, ControlError,
    ControlMethod, ControlRequest, ControlResponse, LineFrameDecoder, decode_control_request,
    decode_control_response, encode_control_request, encode_control_response,
};
pub use ids::{GroupId, PaneId, SplitId, SurfaceId, TabId, WorkspaceId};
pub use model::{
    AppModel, CloseOutcome, CloseScope, GroupModel, ModelError, RuntimeLocation,
    RuntimePresentation, TabModel, WorkspaceModel,
};
pub use platform::{
    AtomicWriteError, FileChange, FileFingerprint, PollingFileWatcher, RuntimePathError,
    UnixSocketAddress, XdgPaths, atomic_write_private, read_bounded, sha256_bytes, sha256_file,
};
pub use runtime::{
    BrowserRuntime, MockBrowserRuntime, MockRuntimeProbe, MockRuntimeSnapshot, MockTerminalRuntime,
    PaneContainer, PaneContainerError, PaneRuntime, PaneSurface, RuntimeKind,
};
pub use settings::{
    BrowserSearchEngine, RestoreLayoutPolicy, SettingsCodecError, SettingsDocument,
    ValidatedSettings, WorkspaceActiveStyle, WorkspaceColorStyle, decode_settings, encode_settings,
};
pub use split::{
    Direction, PixelRect, PixelSize, ResizeTarget, Split, SplitAxis, SplitLayout, SplitNode,
    directional_neighbor,
};
pub use state::{
    AppSnapshot, PaneContentKind, PaneDetail, PaneSurfaceDetail, SnapshotCodecError,
    TabGroupSnapshot, TerminalTabSnapshot, WorkspaceSnapshot, decode_snapshot, encode_snapshot,
    valid_resume_command,
};
