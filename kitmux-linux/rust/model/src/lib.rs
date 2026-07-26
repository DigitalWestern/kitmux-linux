//! Display-free Kitmux product model.
//!
//! This crate owns identity, hierarchy, split geometry, navigation, close
//! cascading, and the lifetime of abstract pane runtimes. It deliberately has
//! no GTK, WebKit, libkitty, filesystem, shell, or network dependency.

mod ids;
mod model;
mod runtime;
mod split;

pub use ids::{GroupId, PaneId, SplitId, SurfaceId, TabId, WorkspaceId};
pub use model::{
    AppModel, CloseOutcome, CloseScope, GroupModel, ModelError, RuntimeLocation,
    RuntimePresentation, TabModel, WorkspaceModel,
};
pub use runtime::{
    BrowserRuntime, MockBrowserRuntime, MockRuntimeProbe, MockRuntimeSnapshot, MockTerminalRuntime,
    PaneContainer, PaneContainerError, PaneRuntime, PaneSurface, RuntimeKind,
};
pub use split::{
    Direction, PixelRect, PixelSize, ResizeTarget, Split, SplitAxis, SplitLayout, SplitNode,
    directional_neighbor,
};
