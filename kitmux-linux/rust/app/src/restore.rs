use kitmux_model::{
    AppModel, AppSnapshot, GroupId, GroupModel, PaneContainer, PaneContentKind, PaneRuntime,
    PaneSurface, ResumeCommandIdentity, SurfaceId, TabId, TabModel, WorkspaceModel,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::runtime::valid_restored_cwd;
use crate::terminal::PendingTerminalRuntime;

pub(crate) struct RestoredProduct {
    pub(crate) navigation: AppModel,
    pub(crate) active_surface: SurfaceId,
    pub(crate) surface_cwds: HashMap<SurfaceId, PathBuf>,
    pub(crate) valid_restored_cwds: HashMap<SurfaceId, PathBuf>,
    pub(crate) surface_resume_commands: HashMap<SurfaceId, String>,
    pub(crate) surface_ssh_profiles: HashMap<SurfaceId, Uuid>,
    pub(crate) resume_offers: Vec<ResumeOffer>,
    pub(crate) created_workspaces: usize,
    pub(crate) created_groups: usize,
}

pub(crate) struct ResumeOffer {
    pub(crate) identity: ResumeCommandIdentity,
    pub(crate) location: String,
}

pub(crate) fn restored_product(snapshot: &AppSnapshot, home: &Path) -> Option<RestoredProduct> {
    let mut surface_ids = HashSet::new();
    let mut surface_cwds = HashMap::new();
    let mut valid_restored_cwds = HashMap::new();
    let mut surface_resume_commands = HashMap::new();
    let mut surface_ssh_profiles = HashMap::new();
    let mut resume_offers = Vec::new();
    let mut workspaces = Vec::with_capacity(snapshot.workspaces.len());
    for workspace in &snapshot.workspaces {
        let mut groups = Vec::with_capacity(workspace.tab_groups.len());
        for group in &workspace.tab_groups {
            let mut tabs = Vec::with_capacity(group.terminal_tabs.len());
            for (tab_index, tab) in group.terminal_tabs.iter().enumerate() {
                let mut panes = Vec::new();
                for (pane_index, pane_id) in tab.root.pane_ids().into_iter().enumerate() {
                    let detail = tab
                        .pane_details
                        .as_ref()
                        .and_then(|details| details.get(&pane_id.to_string()));
                    let mut surfaces = Vec::new();
                    let mut active_surface_index = 0;
                    if let Some(stack) = detail.and_then(|detail| detail.surfaces.as_ref()) {
                        let saved_active = detail
                            .and_then(|detail| detail.active_surface_index)
                            .unwrap_or(0) as usize;
                        for (index, saved) in stack.iter().enumerate() {
                            if saved.kind != PaneContentKind::Terminal {
                                continue;
                            }
                            let mut id = SurfaceId::from_uuid(saved.id);
                            while !surface_ids.insert(id) {
                                id = SurfaceId::new();
                            }
                            if index == saved_active {
                                active_surface_index = surfaces.len();
                            }
                            let saved_cwd = saved
                                .cwd
                                .as_deref()
                                .map(PathBuf::from)
                                .filter(|path| valid_restored_cwd(path));
                            let cwd = saved_cwd.clone().unwrap_or_else(|| home.to_owned());
                            let cwd_label = cwd.to_string_lossy().into_owned();
                            surface_cwds.insert(id, cwd);
                            if let Some(cwd) = saved_cwd {
                                valid_restored_cwds.insert(id, cwd);
                            }
                            if let Some(command) = saved.resume_command.clone() {
                                surface_resume_commands.insert(id, command.clone());
                                resume_offers.push(ResumeOffer {
                                    identity: ResumeCommandIdentity {
                                        pane_id,
                                        surface_id: id,
                                        command,
                                        cwd: Some(cwd_label),
                                    },
                                    location: format!(
                                        "{} ▸ {} ▸ tab {} ▸ pane {} ▸ surface {}",
                                        workspace.name,
                                        group.name,
                                        tab_index + 1,
                                        pane_index + 1,
                                        surfaces.len() + 1
                                    ),
                                });
                            }
                            surfaces.push(PaneSurface::new(
                                id,
                                PaneRuntime::Terminal(Box::new(PendingTerminalRuntime {
                                    closed: false,
                                })),
                            ));
                        }
                    }
                    if surfaces.is_empty() {
                        let id = SurfaceId::new();
                        surface_ids.insert(id);
                        let saved_cwd = detail
                            .and_then(|detail| detail.cwd.as_deref())
                            .map(PathBuf::from)
                            .filter(|path| valid_restored_cwd(path));
                        let cwd = saved_cwd.clone().unwrap_or_else(|| home.to_owned());
                        let cwd_label = cwd.to_string_lossy().into_owned();
                        surface_cwds.insert(id, cwd);
                        if let Some(cwd) = saved_cwd {
                            valid_restored_cwds.insert(id, cwd);
                        }
                        if let Some(profile_id) = detail.and_then(|detail| detail.ssh_profile_id) {
                            surface_ssh_profiles.insert(id, profile_id);
                        } else if let Some(command) =
                            detail.and_then(|detail| detail.resume_command.clone())
                        {
                            surface_resume_commands.insert(id, command.clone());
                            resume_offers.push(ResumeOffer {
                                identity: ResumeCommandIdentity {
                                    pane_id,
                                    surface_id: id,
                                    command,
                                    cwd: Some(cwd_label),
                                },
                                location: format!(
                                    "{} ▸ {} ▸ tab {} ▸ pane {}",
                                    workspace.name,
                                    group.name,
                                    tab_index + 1,
                                    pane_index + 1
                                ),
                            });
                        }
                        surfaces.push(PaneSurface::new(
                            id,
                            PaneRuntime::Terminal(Box::new(PendingTerminalRuntime {
                                closed: false,
                            })),
                        ));
                        active_surface_index = 0;
                    }
                    panes.push(PaneContainer::new(pane_id, surfaces, active_surface_index).ok()?);
                }
                let mut model =
                    TabModel::new(TabId::new(), tab.root.clone(), tab.focused_pane_id, panes)
                        .ok()?;
                if let Some(title) = tab.custom_title.as_deref() {
                    model.rename(Some(title));
                }
                tabs.push(model);
            }
            let mut model = GroupModel::new(
                GroupId::new(),
                tabs,
                group.active_terminal_tab_index as usize,
            )
            .ok()?;
            model.rename(&group.name);
            groups.push(model);
        }
        let mut model = WorkspaceModel::new(
            workspace.id.unwrap_or_default(),
            groups,
            workspace.active_tab_group_index as usize,
        )
        .ok()?;
        model.rename(&workspace.name);
        workspaces.push(model);
    }
    let navigation = AppModel::new(workspaces, snapshot.active_workspace_index as usize).ok()?;
    let active_surface = navigation
        .active_tab()
        .pane(navigation.active_tab().focused_pane_id())?
        .active_surface()
        .id();
    Some(RestoredProduct {
        navigation,
        active_surface,
        surface_cwds,
        valid_restored_cwds,
        surface_resume_commands,
        surface_ssh_profiles,
        resume_offers,
        created_workspaces: snapshot.created_workspace_count.max(1) as usize,
        created_groups: snapshot
            .workspaces
            .iter()
            .map(|workspace| workspace.created_group_count)
            .max()
            .unwrap_or(1)
            .max(1) as usize,
    })
}
