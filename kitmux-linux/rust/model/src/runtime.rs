use crate::{PaneId, SurfaceId};
use std::fmt;
use std::sync::{Arc, Mutex};

pub trait TerminalRuntime: Send {
    fn pump(&mut self);
    fn close(&mut self);
    fn is_closed(&self) -> bool;
}

pub trait BrowserRuntime: Send {
    fn close(&mut self);
    fn is_closed(&self) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeKind {
    Terminal,
    Browser,
}

pub enum PaneRuntime {
    Terminal(Box<dyn TerminalRuntime>),
    Browser(Box<dyn BrowserRuntime>),
}

impl PaneRuntime {
    #[must_use]
    pub const fn kind(&self) -> RuntimeKind {
        match self {
            Self::Terminal(_) => RuntimeKind::Terminal,
            Self::Browser(_) => RuntimeKind::Browser,
        }
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        match self {
            Self::Terminal(runtime) => runtime.is_closed(),
            Self::Browser(runtime) => runtime.is_closed(),
        }
    }

    pub fn pump_if_terminal(&mut self) {
        if let Self::Terminal(runtime) = self
            && !runtime.is_closed()
        {
            runtime.pump();
        }
    }

    pub fn close(&mut self) {
        match self {
            Self::Terminal(runtime) if !runtime.is_closed() => runtime.close(),
            Self::Browser(runtime) if !runtime.is_closed() => runtime.close(),
            _ => {}
        }
    }
}

pub struct PaneSurface {
    id: SurfaceId,
    runtime: PaneRuntime,
}

impl PaneSurface {
    #[must_use]
    pub const fn new(id: SurfaceId, runtime: PaneRuntime) -> Self {
        Self { id, runtime }
    }

    #[must_use]
    pub fn with_new_id(runtime: PaneRuntime) -> Self {
        Self::new(SurfaceId::new(), runtime)
    }

    #[must_use]
    pub const fn id(&self) -> SurfaceId {
        self.id
    }

    #[must_use]
    pub const fn kind(&self) -> RuntimeKind {
        self.runtime.kind()
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.runtime.is_closed()
    }

    pub(crate) fn pump_if_terminal(&mut self) {
        self.runtime.pump_if_terminal();
    }

    pub(crate) fn close(&mut self) {
        self.runtime.close();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneContainerError {
    EmptySurfaceStack,
    DuplicateSurfaceId(SurfaceId),
}

impl fmt::Display for PaneContainerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySurfaceStack => formatter.write_str("pane surface stack cannot be empty"),
            Self::DuplicateSurfaceId(id) => write!(formatter, "duplicate surface ID {id}"),
        }
    }
}

impl std::error::Error for PaneContainerError {}

pub struct PaneContainer {
    id: PaneId,
    surfaces: Vec<PaneSurface>,
    active_surface_index: usize,
}

impl PaneContainer {
    pub fn new(
        id: PaneId,
        surfaces: Vec<PaneSurface>,
        active_surface_index: usize,
    ) -> Result<Self, PaneContainerError> {
        if surfaces.is_empty() {
            return Err(PaneContainerError::EmptySurfaceStack);
        }
        for (index, surface) in surfaces.iter().enumerate() {
            if surfaces[..index]
                .iter()
                .any(|candidate| candidate.id == surface.id)
            {
                return Err(PaneContainerError::DuplicateSurfaceId(surface.id));
            }
        }
        Ok(Self {
            id,
            active_surface_index: active_surface_index.min(surfaces.len() - 1),
            surfaces,
        })
    }

    pub fn single(id: PaneId, runtime: PaneRuntime) -> Self {
        Self::new(id, vec![PaneSurface::with_new_id(runtime)], 0)
            .expect("a one-surface pane is always valid")
    }

    #[must_use]
    pub const fn id(&self) -> PaneId {
        self.id
    }

    #[must_use]
    pub fn surfaces(&self) -> &[PaneSurface] {
        &self.surfaces
    }

    #[must_use]
    pub const fn active_surface_index(&self) -> usize {
        self.active_surface_index
    }

    #[must_use]
    pub fn active_surface(&self) -> &PaneSurface {
        &self.surfaces[self.active_surface_index]
    }

    pub fn add(&mut self, surface: PaneSurface) -> Result<usize, PaneContainerError> {
        if self
            .surfaces
            .iter()
            .any(|candidate| candidate.id == surface.id)
        {
            return Err(PaneContainerError::DuplicateSurfaceId(surface.id));
        }
        self.surfaces.push(surface);
        self.active_surface_index = self.surfaces.len() - 1;
        Ok(self.active_surface_index)
    }

    pub fn select(&mut self, surface_id: SurfaceId) -> bool {
        let Some(index) = self
            .surfaces
            .iter()
            .position(|surface| surface.id == surface_id)
        else {
            return false;
        };
        self.active_surface_index = index;
        true
    }

    pub fn remove(&mut self, surface_id: SurfaceId) -> Option<PaneSurface> {
        if self.surfaces.len() <= 1 {
            return None;
        }
        let index = self
            .surfaces
            .iter()
            .position(|surface| surface.id == surface_id)?;
        let removed = self.surfaces.remove(index);
        if self.active_surface_index >= self.surfaces.len() {
            self.active_surface_index = self.surfaces.len() - 1;
        } else if index < self.active_surface_index {
            self.active_surface_index -= 1;
        }
        Some(removed)
    }

    pub(crate) fn pump_terminals(&mut self) {
        for surface in &mut self.surfaces {
            surface.pump_if_terminal();
        }
    }

    pub(crate) fn close_all(&mut self) {
        for surface in &mut self.surfaces {
            surface.close();
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MockRuntimeSnapshot {
    pub pump_count: u64,
    pub close_count: u64,
    pub closed: bool,
}

#[derive(Clone, Default)]
pub struct MockRuntimeProbe(Arc<Mutex<MockRuntimeSnapshot>>);

impl MockRuntimeProbe {
    #[must_use]
    pub fn snapshot(&self) -> MockRuntimeSnapshot {
        *self.0.lock().expect("mock runtime probe lock poisoned")
    }

    fn pump(&self) {
        let mut state = self.0.lock().expect("mock runtime probe lock poisoned");
        state.pump_count += 1;
    }

    fn close(&self) {
        let mut state = self.0.lock().expect("mock runtime probe lock poisoned");
        state.close_count += 1;
        state.closed = true;
    }

    fn is_closed(&self) -> bool {
        self.snapshot().closed
    }
}

pub struct MockTerminalRuntime {
    probe: MockRuntimeProbe,
}

impl MockTerminalRuntime {
    #[must_use]
    pub fn new() -> (Self, MockRuntimeProbe) {
        let probe = MockRuntimeProbe::default();
        (
            Self {
                probe: probe.clone(),
            },
            probe,
        )
    }
}

impl TerminalRuntime for MockTerminalRuntime {
    fn pump(&mut self) {
        self.probe.pump();
    }

    fn close(&mut self) {
        self.probe.close();
    }

    fn is_closed(&self) -> bool {
        self.probe.is_closed()
    }
}

pub struct MockBrowserRuntime {
    probe: MockRuntimeProbe,
}

impl MockBrowserRuntime {
    #[must_use]
    pub fn new() -> (Self, MockRuntimeProbe) {
        let probe = MockRuntimeProbe::default();
        (
            Self {
                probe: probe.clone(),
            },
            probe,
        )
    }
}

impl BrowserRuntime for MockBrowserRuntime {
    fn close(&mut self) {
        self.probe.close();
    }

    fn is_closed(&self) -> bool {
        self.probe.is_closed()
    }
}
