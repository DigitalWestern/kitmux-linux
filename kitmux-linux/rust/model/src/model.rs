use crate::{
    Direction, GroupId, PaneContainer, PaneContainerError, PaneId, PaneSurface, PixelRect,
    PixelSize, RuntimeKind, SplitAxis, SplitId, SplitLayout, SplitNode, SurfaceId, TabId,
    WorkspaceId, directional_neighbor,
};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelError {
    EmptyHierarchy(&'static str),
    ActiveIndexOutOfBounds(&'static str),
    DuplicateId(&'static str),
    InvalidSplitTree,
    PaneRegistryMismatch,
    UnknownPane(PaneId),
    DuplicatePane(PaneId),
    Surface(PaneContainerError),
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyHierarchy(level) => write!(formatter, "{level} cannot be empty"),
            Self::ActiveIndexOutOfBounds(level) => {
                write!(formatter, "active {level} index is out of bounds")
            }
            Self::DuplicateId(kind) => write!(formatter, "duplicate {kind} ID"),
            Self::InvalidSplitTree => formatter.write_str("split tree violates model invariants"),
            Self::PaneRegistryMismatch => {
                formatter.write_str("split leaves and pane registry do not match")
            }
            Self::UnknownPane(id) => write!(formatter, "unknown pane ID {id}"),
            Self::DuplicatePane(id) => write!(formatter, "duplicate pane ID {id}"),
            Self::Surface(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ModelError {}

impl From<PaneContainerError> for ModelError {
    fn from(value: PaneContainerError) -> Self {
        Self::Surface(value)
    }
}

pub struct TabModel {
    id: TabId,
    custom_title: Option<String>,
    root: SplitNode,
    focused_pane_id: PaneId,
    panes: HashMap<PaneId, PaneContainer>,
}

impl TabModel {
    pub fn new(
        id: TabId,
        root: SplitNode,
        focused_pane_id: PaneId,
        panes: Vec<PaneContainer>,
    ) -> Result<Self, ModelError> {
        if !root.has_unique_ids_and_valid_ratios() {
            return Err(ModelError::InvalidSplitTree);
        }
        let mut pane_registry = HashMap::new();
        for pane in panes {
            let id = pane.id();
            if pane_registry.insert(id, pane).is_some() {
                return Err(ModelError::DuplicatePane(id));
            }
        }
        let leaf_ids: HashSet<_> = root.pane_ids().into_iter().collect();
        let registry_ids: HashSet<_> = pane_registry.keys().copied().collect();
        if leaf_ids != registry_ids {
            return Err(ModelError::PaneRegistryMismatch);
        }
        if !leaf_ids.contains(&focused_pane_id) {
            return Err(ModelError::UnknownPane(focused_pane_id));
        }
        Ok(Self {
            id,
            custom_title: None,
            root,
            focused_pane_id,
            panes: pane_registry,
        })
    }

    pub fn single(id: TabId, pane: PaneContainer) -> Self {
        let pane_id = pane.id();
        Self::new(id, SplitNode::pane(pane_id), pane_id, vec![pane])
            .expect("a one-pane tab is always valid")
    }

    #[must_use]
    pub const fn id(&self) -> TabId {
        self.id
    }

    #[must_use]
    pub fn custom_title(&self) -> Option<&str> {
        self.custom_title.as_deref()
    }

    pub fn rename(&mut self, title: Option<&str>) -> bool {
        let title = title.and_then(normalized_optional_label);
        if self.custom_title == title {
            return false;
        }
        self.custom_title = title;
        true
    }

    #[must_use]
    pub const fn root(&self) -> &SplitNode {
        &self.root
    }

    #[must_use]
    pub const fn focused_pane_id(&self) -> PaneId {
        self.focused_pane_id
    }

    #[must_use]
    pub fn pane_ids(&self) -> Vec<PaneId> {
        self.root.pane_ids()
    }

    #[must_use]
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    #[must_use]
    pub fn pane(&self, id: PaneId) -> Option<&PaneContainer> {
        self.panes.get(&id)
    }

    #[must_use]
    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut PaneContainer> {
        self.panes.get_mut(&id)
    }

    pub fn focus(&mut self, pane_id: PaneId) -> bool {
        if !self.panes.contains_key(&pane_id) {
            return false;
        }
        self.focused_pane_id = pane_id;
        true
    }

    pub fn cycle_focus(&mut self, direction: i32) -> bool {
        let pane_ids = self.pane_ids();
        if pane_ids.len() <= 1 {
            return false;
        }
        let Some(index) = pane_ids.iter().position(|id| *id == self.focused_pane_id) else {
            return false;
        };
        self.focused_pane_id = pane_ids[wrapped_index(index, direction, pane_ids.len())];
        true
    }

    pub fn move_focus(&mut self, direction: Direction, layout: &SplitLayout) -> bool {
        let pane_ids = self.pane_ids();
        let Some(neighbor) = directional_neighbor(
            self.focused_pane_id,
            direction,
            &layout.pane_frames,
            &pane_ids,
        ) else {
            return false;
        };
        self.focused_pane_id = neighbor;
        true
    }

    pub fn resize_focused(
        &mut self,
        direction: Direction,
        rect: PixelRect,
        gap: i32,
        minimum_leaf_size: PixelSize,
        ratio_step: f64,
    ) -> bool {
        let Some(target) = self.root.resize_target(self.focused_pane_id, direction) else {
            return false;
        };
        let layout = self.layout(rect, gap, minimum_leaf_size);
        let Some(split_rect) = layout.split_frames.get(&target.split_id).copied() else {
            return false;
        };
        let Some(bounds) =
            self.root
                .ratio_bounds(target.split_id, split_rect, gap, minimum_leaf_size)
        else {
            return false;
        };
        self.root
            .adjust_ratio(target.split_id, target.ratio_delta(ratio_step), bounds)
    }

    pub fn set_split_ratio(
        &mut self,
        split_id: SplitId,
        ratio: f64,
        split_rect: PixelRect,
        gap: i32,
        minimum_leaf_size: PixelSize,
    ) -> bool {
        let Some(bounds) = self
            .root
            .ratio_bounds(split_id, split_rect, gap, minimum_leaf_size)
        else {
            return false;
        };
        self.root.set_ratio(split_id, ratio, bounds)
    }

    pub fn split_pane(
        &mut self,
        pane_id: PaneId,
        axis: SplitAxis,
        new_pane: PaneContainer,
    ) -> Result<(), ModelError> {
        if !self.panes.contains_key(&pane_id) {
            return Err(ModelError::UnknownPane(pane_id));
        }
        let new_id = new_pane.id();
        if self.panes.contains_key(&new_id) {
            return Err(ModelError::DuplicatePane(new_id));
        }
        if !self.root.split_pane(pane_id, axis, new_id) {
            return Err(ModelError::InvalidSplitTree);
        }
        self.panes.insert(new_id, new_pane);
        self.focused_pane_id = new_id;
        Ok(())
    }

    pub fn swap_panes(&mut self, first: PaneId, second: PaneId) -> bool {
        self.root.swap_panes(first, second)
    }

    pub fn add_surface(
        &mut self,
        pane_id: PaneId,
        surface: PaneSurface,
    ) -> Result<usize, ModelError> {
        self.panes
            .get_mut(&pane_id)
            .ok_or(ModelError::UnknownPane(pane_id))?
            .add(surface)
            .map_err(Into::into)
    }

    pub fn select_surface(&mut self, pane_id: PaneId, surface_id: SurfaceId) -> bool {
        self.panes
            .get_mut(&pane_id)
            .is_some_and(|pane| pane.select(surface_id))
    }

    #[must_use]
    pub fn layout(&self, rect: PixelRect, gap: i32, minimum_leaf_size: PixelSize) -> SplitLayout {
        self.root.layout(rect, gap, minimum_leaf_size)
    }

    fn remove_pane(&mut self, pane_id: PaneId) -> Option<PaneContainer> {
        if self.panes.len() <= 1 {
            return None;
        }
        let old_order = self.pane_ids();
        let old_index = old_order.iter().position(|id| *id == pane_id)?;
        let new_root = self.root.removing_pane(pane_id)?;
        let replacement = if old_index + 1 < old_order.len() {
            old_order[old_index + 1]
        } else {
            old_order[old_index - 1]
        };
        self.root = new_root;
        if self.focused_pane_id == pane_id {
            self.focused_pane_id = replacement;
        }
        self.panes.remove(&pane_id)
    }

    fn pump_terminals(&mut self) {
        for pane in self.panes.values_mut() {
            pane.pump_terminals();
        }
    }

    fn close_all(&mut self) {
        for pane in self.panes.values_mut() {
            pane.close_all();
        }
    }
}

pub struct GroupModel {
    id: GroupId,
    name: String,
    tabs: Vec<TabModel>,
    active_tab_index: usize,
}

impl GroupModel {
    pub fn new(
        id: GroupId,
        tabs: Vec<TabModel>,
        active_tab_index: usize,
    ) -> Result<Self, ModelError> {
        validate_non_empty_active(&tabs, active_tab_index, "tab")?;
        if !all_unique(tabs.iter().map(TabModel::id)) {
            return Err(ModelError::DuplicateId("tab"));
        }
        Ok(Self {
            id,
            name: "Group 1".to_owned(),
            tabs,
            active_tab_index,
        })
    }

    pub fn single(id: GroupId, tab: TabModel) -> Self {
        Self::new(id, vec![tab], 0).expect("a one-tab group is always valid")
    }

    #[must_use]
    pub const fn id(&self) -> GroupId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn rename(&mut self, name: &str) -> bool {
        let Some(name) = normalized_required_label(name) else {
            return false;
        };
        if self.name == name {
            return false;
        }
        self.name = name;
        true
    }

    #[must_use]
    pub fn tabs(&self) -> &[TabModel] {
        &self.tabs
    }

    #[must_use]
    pub const fn active_tab_index(&self) -> usize {
        self.active_tab_index
    }

    #[must_use]
    pub fn active_tab(&self) -> &TabModel {
        &self.tabs[self.active_tab_index]
    }

    #[must_use]
    pub fn active_tab_mut(&mut self) -> &mut TabModel {
        &mut self.tabs[self.active_tab_index]
    }

    pub fn select_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        self.active_tab_index = index;
        true
    }

    pub fn cycle_tab(&mut self, direction: i32) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }
        self.active_tab_index = wrapped_index(self.active_tab_index, direction, self.tabs.len());
        true
    }

    pub fn append_tab(&mut self, tab: TabModel) -> Result<usize, ModelError> {
        if self.tabs.iter().any(|candidate| candidate.id == tab.id) {
            return Err(ModelError::DuplicateId("tab"));
        }
        self.tabs.push(tab);
        self.active_tab_index = self.tabs.len() - 1;
        Ok(self.active_tab_index)
    }

    pub fn move_tab(&mut self, id: TabId, target_index: usize) -> bool {
        move_selected(
            &mut self.tabs,
            &mut self.active_tab_index,
            id,
            target_index,
            TabModel::id,
        )
    }

    pub fn close_tab(&mut self, index: usize) -> Option<TabId> {
        let mut removed = self.remove_tab(index)?;
        let id = removed.id();
        removed.close_all();
        Some(id)
    }

    fn remove_tab(&mut self, index: usize) -> Option<TabModel> {
        remove_selected(&mut self.tabs, &mut self.active_tab_index, index)
    }

    fn close_all(&mut self) {
        for tab in &mut self.tabs {
            tab.close_all();
        }
    }
}

pub struct WorkspaceModel {
    id: WorkspaceId,
    name: String,
    groups: Vec<GroupModel>,
    active_group_index: usize,
}

impl WorkspaceModel {
    pub fn new(
        id: WorkspaceId,
        groups: Vec<GroupModel>,
        active_group_index: usize,
    ) -> Result<Self, ModelError> {
        validate_non_empty_active(&groups, active_group_index, "group")?;
        if !all_unique(groups.iter().map(GroupModel::id)) {
            return Err(ModelError::DuplicateId("group"));
        }
        Ok(Self {
            id,
            name: "Workspace 1".to_owned(),
            groups,
            active_group_index,
        })
    }

    pub fn single(id: WorkspaceId, group: GroupModel) -> Self {
        Self::new(id, vec![group], 0).expect("a one-group workspace is always valid")
    }

    #[must_use]
    pub const fn id(&self) -> WorkspaceId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn rename(&mut self, name: &str) -> bool {
        let Some(name) = normalized_required_label(name) else {
            return false;
        };
        if self.name == name {
            return false;
        }
        self.name = name;
        true
    }

    #[must_use]
    pub fn groups(&self) -> &[GroupModel] {
        &self.groups
    }

    #[must_use]
    pub const fn active_group_index(&self) -> usize {
        self.active_group_index
    }

    #[must_use]
    pub fn active_group(&self) -> &GroupModel {
        &self.groups[self.active_group_index]
    }

    #[must_use]
    pub fn active_group_mut(&mut self) -> &mut GroupModel {
        &mut self.groups[self.active_group_index]
    }

    pub fn select_group(&mut self, index: usize) -> bool {
        if index >= self.groups.len() {
            return false;
        }
        self.active_group_index = index;
        true
    }

    pub fn cycle_group(&mut self, direction: i32) -> bool {
        if self.groups.len() <= 1 {
            return false;
        }
        self.active_group_index =
            wrapped_index(self.active_group_index, direction, self.groups.len());
        true
    }

    pub fn append_group(&mut self, group: GroupModel) -> Result<usize, ModelError> {
        if self.groups.iter().any(|candidate| candidate.id == group.id) {
            return Err(ModelError::DuplicateId("group"));
        }
        self.groups.push(group);
        self.active_group_index = self.groups.len() - 1;
        Ok(self.active_group_index)
    }

    pub fn move_group(&mut self, id: GroupId, target_index: usize) -> bool {
        move_selected(
            &mut self.groups,
            &mut self.active_group_index,
            id,
            target_index,
            GroupModel::id,
        )
    }

    pub fn close_group(&mut self, index: usize) -> Option<GroupId> {
        let mut removed = self.remove_group(index)?;
        let id = removed.id();
        removed.close_all();
        Some(id)
    }

    fn remove_group(&mut self, index: usize) -> Option<GroupModel> {
        remove_selected(&mut self.groups, &mut self.active_group_index, index)
    }

    fn close_all(&mut self) {
        for group in &mut self.groups {
            group.close_all();
        }
    }
}

pub struct AppModel {
    workspaces: Vec<WorkspaceModel>,
    active_workspace_index: usize,
}

impl AppModel {
    pub fn new(
        workspaces: Vec<WorkspaceModel>,
        active_workspace_index: usize,
    ) -> Result<Self, ModelError> {
        validate_non_empty_active(&workspaces, active_workspace_index, "workspace")?;
        let model = Self {
            workspaces,
            active_workspace_index,
        };
        model.validate_global_ids()?;
        Ok(model)
    }

    pub fn single(workspace: WorkspaceModel) -> Self {
        Self::new(vec![workspace], 0).expect("a one-workspace app is always valid")
    }

    #[must_use]
    pub fn workspaces(&self) -> &[WorkspaceModel] {
        &self.workspaces
    }

    #[must_use]
    pub const fn active_workspace_index(&self) -> usize {
        self.active_workspace_index
    }

    #[must_use]
    pub fn active_workspace(&self) -> &WorkspaceModel {
        &self.workspaces[self.active_workspace_index]
    }

    #[must_use]
    pub fn active_workspace_mut(&mut self) -> &mut WorkspaceModel {
        &mut self.workspaces[self.active_workspace_index]
    }

    #[must_use]
    pub fn active_tab(&self) -> &TabModel {
        self.active_workspace().active_group().active_tab()
    }

    #[must_use]
    pub fn active_tab_mut(&mut self) -> &mut TabModel {
        self.active_workspace_mut()
            .active_group_mut()
            .active_tab_mut()
    }

    pub fn select_workspace(&mut self, index: usize) -> bool {
        if index >= self.workspaces.len() {
            return false;
        }
        self.active_workspace_index = index;
        true
    }

    pub fn cycle_workspace(&mut self, direction: i32) -> bool {
        if self.workspaces.len() <= 1 {
            return false;
        }
        self.active_workspace_index = wrapped_index(
            self.active_workspace_index,
            direction,
            self.workspaces.len(),
        );
        true
    }

    pub fn append_workspace(&mut self, workspace: WorkspaceModel) -> Result<usize, ModelError> {
        let id = workspace.id;
        if self.workspaces.iter().any(|candidate| candidate.id == id) {
            return Err(ModelError::DuplicateId("workspace"));
        }
        self.workspaces.push(workspace);
        if let Err(error) = self.validate_global_ids() {
            self.workspaces.pop();
            return Err(error);
        }
        self.active_workspace_index = self.workspaces.len() - 1;
        Ok(self.active_workspace_index)
    }

    pub fn move_workspace(&mut self, id: WorkspaceId, target_index: usize) -> bool {
        move_selected(
            &mut self.workspaces,
            &mut self.active_workspace_index,
            id,
            target_index,
            WorkspaceModel::id,
        )
    }

    pub fn close_workspace(&mut self, index: usize) -> Option<WorkspaceId> {
        let mut removed = remove_selected(
            &mut self.workspaces,
            &mut self.active_workspace_index,
            index,
        )?;
        let id = removed.id();
        removed.close_all();
        Some(id)
    }

    pub fn focus_pane(&mut self, pane_id: PaneId) -> bool {
        let Some((workspace, group, tab)) = self.find_pane_location(pane_id) else {
            return false;
        };
        self.active_workspace_index = workspace;
        self.workspaces[workspace].active_group_index = group;
        self.workspaces[workspace].groups[group].active_tab_index = tab;
        self.workspaces[workspace].groups[group].tabs[tab].focus(pane_id)
    }

    #[must_use]
    pub fn resolve_close_scope(&self, pane_id: PaneId) -> Option<CloseScope> {
        let (workspace_index, group_index, tab_index) = self.find_pane_location(pane_id)?;
        let workspace = &self.workspaces[workspace_index];
        let group = &workspace.groups[group_index];
        let tab = &group.tabs[tab_index];
        if tab.pane_count() > 1 {
            Some(CloseScope::Pane(pane_id))
        } else if group.tabs.len() > 1 {
            Some(CloseScope::Tab(tab.id))
        } else if workspace.groups.len() > 1 {
            Some(CloseScope::Group(group.id))
        } else {
            Some(CloseScope::Workspace(workspace.id))
        }
    }

    pub fn close_pane(&mut self, pane_id: PaneId) -> Option<CloseOutcome> {
        let (workspace_index, group_index, tab_index) = self.find_pane_location(pane_id)?;
        let scope = self.resolve_close_scope(pane_id)?;
        match scope {
            CloseScope::Pane(_) => {
                let mut removed = self.workspaces[workspace_index].groups[group_index].tabs
                    [tab_index]
                    .remove_pane(pane_id)?;
                removed.close_all();
                Some(CloseOutcome::Removed(scope))
            }
            CloseScope::Tab(_) => {
                let mut removed =
                    self.workspaces[workspace_index].groups[group_index].remove_tab(tab_index)?;
                removed.close_all();
                Some(CloseOutcome::Removed(scope))
            }
            CloseScope::Group(_) => {
                let mut removed = self.workspaces[workspace_index].remove_group(group_index)?;
                removed.close_all();
                Some(CloseOutcome::Removed(scope))
            }
            CloseScope::Workspace(_) if self.workspaces.len() > 1 => {
                let mut removed = remove_selected(
                    &mut self.workspaces,
                    &mut self.active_workspace_index,
                    workspace_index,
                )?;
                removed.close_all();
                Some(CloseOutcome::Removed(scope))
            }
            CloseScope::Workspace(_) => Some(CloseOutcome::HostCloseRequired(scope)),
        }
    }

    pub fn pump_terminals(&mut self) {
        for workspace in &mut self.workspaces {
            for group in &mut workspace.groups {
                for tab in &mut group.tabs {
                    tab.pump_terminals();
                }
            }
        }
    }

    #[must_use]
    pub fn runtime_presentations(&self) -> Vec<RuntimePresentation> {
        let mut result = Vec::new();
        for (workspace_index, workspace) in self.workspaces.iter().enumerate() {
            for (group_index, group) in workspace.groups.iter().enumerate() {
                for (tab_index, tab) in group.tabs.iter().enumerate() {
                    let layout_visible = workspace_index == self.active_workspace_index
                        && group_index == workspace.active_group_index
                        && tab_index == group.active_tab_index;
                    for pane_id in tab.pane_ids() {
                        let pane = &tab.panes[&pane_id];
                        for (surface_index, surface) in pane.surfaces().iter().enumerate() {
                            let surface_visible =
                                layout_visible && surface_index == pane.active_surface_index();
                            result.push(RuntimePresentation {
                                location: RuntimeLocation {
                                    workspace_id: workspace.id,
                                    group_id: group.id,
                                    tab_id: tab.id,
                                    pane_id,
                                    surface_id: surface.id(),
                                },
                                kind: surface.kind(),
                                layout_visible,
                                surface_visible,
                                accepts_input: surface_visible && pane_id == tab.focused_pane_id,
                                pumps_while_hidden: surface.kind() == RuntimeKind::Terminal
                                    && !surface.is_closed(),
                            });
                        }
                    }
                }
            }
        }
        result
    }

    fn find_pane_location(&self, pane_id: PaneId) -> Option<(usize, usize, usize)> {
        for (workspace_index, workspace) in self.workspaces.iter().enumerate() {
            for (group_index, group) in workspace.groups.iter().enumerate() {
                for (tab_index, tab) in group.tabs.iter().enumerate() {
                    if tab.panes.contains_key(&pane_id) {
                        return Some((workspace_index, group_index, tab_index));
                    }
                }
            }
        }
        None
    }

    fn validate_global_ids(&self) -> Result<(), ModelError> {
        if !all_unique(self.workspaces.iter().map(WorkspaceModel::id)) {
            return Err(ModelError::DuplicateId("workspace"));
        }
        let mut group_ids = HashSet::new();
        let mut tab_ids = HashSet::new();
        let mut pane_ids = HashSet::new();
        let mut surface_ids = HashSet::new();
        for workspace in &self.workspaces {
            for group in &workspace.groups {
                if !group_ids.insert(group.id) {
                    return Err(ModelError::DuplicateId("group"));
                }
                for tab in &group.tabs {
                    if !tab_ids.insert(tab.id) {
                        return Err(ModelError::DuplicateId("tab"));
                    }
                    for pane in tab.panes.values() {
                        if !pane_ids.insert(pane.id()) {
                            return Err(ModelError::DuplicateId("pane"));
                        }
                        for surface in pane.surfaces() {
                            if !surface_ids.insert(surface.id()) {
                                return Err(ModelError::DuplicateId("surface"));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseScope {
    Pane(PaneId),
    Tab(TabId),
    Group(GroupId),
    Workspace(WorkspaceId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseOutcome {
    Removed(CloseScope),
    HostCloseRequired(CloseScope),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeLocation {
    pub workspace_id: WorkspaceId,
    pub group_id: GroupId,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimePresentation {
    pub location: RuntimeLocation,
    pub kind: RuntimeKind,
    pub layout_visible: bool,
    pub surface_visible: bool,
    pub accepts_input: bool,
    pub pumps_while_hidden: bool,
}

fn validate_non_empty_active<T>(
    values: &[T],
    active_index: usize,
    level: &'static str,
) -> Result<(), ModelError> {
    if values.is_empty() {
        return Err(ModelError::EmptyHierarchy(level));
    }
    if active_index >= values.len() {
        return Err(ModelError::ActiveIndexOutOfBounds(level));
    }
    Ok(())
}

fn all_unique<T: Eq + std::hash::Hash>(values: impl IntoIterator<Item = T>) -> bool {
    let mut seen = HashSet::new();
    values.into_iter().all(|value| seen.insert(value))
}

fn normalized_optional_label(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && !value.chars().any(char::is_control))
        .then(|| value.chars().take(256).collect())
}

fn normalized_required_label(value: &str) -> Option<String> {
    normalized_optional_label(value)
}

fn wrapped_index(index: usize, direction: i32, count: usize) -> usize {
    let count = count as i64;
    (index as i64 + i64::from(direction)).rem_euclid(count) as usize
}

fn move_selected<T, Id: Copy + Eq>(
    values: &mut Vec<T>,
    active_index: &mut usize,
    id: Id,
    target_index: usize,
    identify: impl Fn(&T) -> Id,
) -> bool {
    if target_index >= values.len() {
        return false;
    }
    let Some(source_index) = values.iter().position(|value| identify(value) == id) else {
        return false;
    };
    if source_index == target_index {
        return false;
    }
    let active_id = identify(&values[*active_index]);
    let moved = values.remove(source_index);
    values.insert(target_index, moved);
    *active_index = values
        .iter()
        .position(|value| identify(value) == active_id)
        .expect("active model remains in the collection");
    true
}

fn remove_selected<T>(
    values: &mut Vec<T>,
    active_index: &mut usize,
    removal_index: usize,
) -> Option<T> {
    if values.len() <= 1 || removal_index >= values.len() {
        return None;
    }
    let active_was_removed = *active_index == removal_index;
    let removed = values.remove(removal_index);
    if active_was_removed {
        *active_index = removal_index.min(values.len() - 1);
    } else if removal_index < *active_index {
        *active_index -= 1;
    }
    Some(removed)
}
