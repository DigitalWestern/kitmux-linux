use kitmux_model::{
    AppModel, CloseOutcome, CloseScope, Direction, GroupId, GroupModel, MockBrowserRuntime,
    MockRuntimeProbe, MockTerminalRuntime, ModelError, PaneContainer, PaneId, PaneRuntime,
    PaneSurface, PixelRect, PixelSize, RuntimeKind, Split, SplitAxis, SplitId, SplitNode,
    SurfaceId, TabId, TabModel, WorkspaceId, WorkspaceModel, directional_neighbor,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

fn terminal_pane(id: PaneId) -> (PaneContainer, MockRuntimeProbe) {
    let (runtime, probe) = MockTerminalRuntime::new();
    (
        PaneContainer::single(id, PaneRuntime::Terminal(Box::new(runtime))),
        probe,
    )
}

fn browser_pane(id: PaneId) -> (PaneContainer, MockRuntimeProbe) {
    let (runtime, probe) = MockBrowserRuntime::new();
    (
        PaneContainer::single(id, PaneRuntime::Browser(Box::new(runtime))),
        probe,
    )
}

fn tab_with_terminal(id: TabId, pane_id: PaneId) -> (TabModel, MockRuntimeProbe) {
    let (pane, probe) = terminal_pane(pane_id);
    (TabModel::single(id, pane), probe)
}

fn group_with_terminal(
    id: GroupId,
    tab_id: TabId,
    pane_id: PaneId,
) -> (GroupModel, MockRuntimeProbe) {
    let (tab, probe) = tab_with_terminal(tab_id, pane_id);
    (GroupModel::single(id, tab), probe)
}

fn workspace_with_terminal(
    id: WorkspaceId,
    group_id: GroupId,
    tab_id: TabId,
    pane_id: PaneId,
) -> (WorkspaceModel, MockRuntimeProbe) {
    let (group, probe) = group_with_terminal(group_id, tab_id, pane_id);
    (WorkspaceModel::single(id, group), probe)
}

#[test]
fn ids_are_stable_distinct_types_with_portable_uuid_encoding() {
    let uuid = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let pane_id = PaneId::from_uuid(uuid);
    let split_id = SplitId::from_uuid(uuid);
    let workspace_id = WorkspaceId::from_uuid(uuid);

    assert_eq!(pane_id.to_string(), uuid.to_string());
    assert_eq!(
        serde_json::to_value(pane_id).unwrap(),
        serde_json::json!({"rawValue": uuid})
    );
    assert_eq!(
        serde_json::to_value(split_id).unwrap(),
        serde_json::json!({"rawValue": uuid})
    );
    assert_eq!(
        serde_json::to_value(workspace_id).unwrap(),
        serde_json::json!(uuid)
    );
}

#[test]
fn split_tree_preserves_depth_first_order_and_collapses_on_close() {
    let a = PaneId::new();
    let b = PaneId::new();
    let c = PaneId::new();
    let mut root = SplitNode::pane(a);

    assert!(root.split_pane(a, SplitAxis::LeftRight, b));
    assert!(root.split_pane(b, SplitAxis::TopBottom, c));
    assert_eq!(root.pane_ids(), vec![a, b, c]);
    assert!(!root.split_pane(a, SplitAxis::TopBottom, c));

    let without_b = root.removing_pane(b).unwrap();
    assert_eq!(without_b.pane_ids(), vec![a, c]);
    assert_eq!(without_b.removing_pane(a), Some(SplitNode::pane(c)));
    assert_eq!(without_b.removing_pane(PaneId::new()), Some(without_b));
}

#[test]
fn split_layout_uses_all_pixels_and_enforces_minimums() {
    let a = PaneId::new();
    let b = PaneId::new();
    let root = SplitNode::Split(Split::new(
        SplitAxis::LeftRight,
        0.01,
        SplitNode::pane(a),
        SplitNode::pane(b),
    ));

    let layout = root.layout(PixelRect::new(0, 0, 300, 101), 6, PixelSize::new(100, 20));
    assert_eq!(layout.pane_frames[&a], PixelRect::new(0, 0, 100, 101));
    assert_eq!(layout.pane_frames[&b], PixelRect::new(106, 0, 194, 101));
    assert_eq!(layout.pane_frames[&b].x + layout.pane_frames[&b].width, 300);
}

#[test]
fn ratio_changes_clamp_to_geometry_bounds_and_ignore_non_finite_values() {
    let a = PaneId::new();
    let b = PaneId::new();
    let split = Split::new(
        SplitAxis::LeftRight,
        0.5,
        SplitNode::pane(a),
        SplitNode::pane(b),
    );
    let id = split.id;
    let mut root = SplitNode::Split(split);
    let bounds = root
        .ratio_bounds(
            id,
            PixelRect::new(0, 0, 300, 100),
            6,
            PixelSize::new(60, 20),
        )
        .unwrap();

    assert!(root.set_ratio(id, 2.0, bounds));
    assert_eq!(root.split(id).unwrap().ratio, bounds.1);
    assert!(root.adjust_ratio(id, -5.0, bounds));
    assert_eq!(root.split(id).unwrap().ratio, bounds.0);
    assert!(root.set_ratio(id, f64::NAN, bounds));
    assert_eq!(root.split(id).unwrap().ratio, bounds.0);
}

#[test]
fn tab_resize_helpers_apply_keyboard_steps_and_pointer_ratios() {
    let a = PaneId::new();
    let b = PaneId::new();
    let split = Split::new(
        SplitAxis::LeftRight,
        0.5,
        SplitNode::pane(a),
        SplitNode::pane(b),
    );
    let split_id = split.id;
    let (pane_a, _) = terminal_pane(a);
    let (pane_b, _) = terminal_pane(b);
    let mut tab = TabModel::new(
        TabId::new(),
        SplitNode::Split(split),
        a,
        vec![pane_a, pane_b],
    )
    .unwrap();
    let rect = PixelRect::new(0, 0, 300, 100);
    let minimum = PixelSize::new(60, 20);

    assert!(tab.resize_focused(Direction::Right, rect, 6, minimum, 0.1));
    assert_eq!(tab.root().split(split_id).unwrap().ratio, 0.6);
    assert!(tab.set_split_ratio(split_id, 2.0, rect, 6, minimum));
    assert!(tab.root().split(split_id).unwrap().ratio < 1.0);
}

#[test]
fn directional_focus_prefers_aligned_nearest_neighbor() {
    let a = PaneId::new();
    let b = PaneId::new();
    let c = PaneId::new();
    let frames = HashMap::from([
        (a, PixelRect::new(0, 0, 100, 100)),
        (b, PixelRect::new(110, 0, 100, 45)),
        (c, PixelRect::new(110, 55, 100, 45)),
    ]);

    assert_eq!(
        directional_neighbor(a, Direction::Right, &frames, &[a, b, c]),
        Some(b)
    );
    assert_eq!(
        directional_neighbor(c, Direction::Up, &frames, &[a, b, c]),
        Some(b)
    );
    assert_eq!(
        directional_neighbor(a, Direction::Left, &frames, &[a, b, c]),
        None
    );
}

#[test]
fn frozen_split_fixture_drives_linux_close_behavior() {
    #[derive(Deserialize)]
    struct Corpus {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Case {
        id: String,
        disposition: String,
        initial: SplitNode,
        #[serde(default)]
        close_order: Vec<String>,
        #[serde(default)]
        expected_after_close: Vec<Option<SplitNode>>,
    }

    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../contracts/fixtures/v1/split-tree.json");
    let corpus: Corpus = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();

    let accepted = corpus
        .cases
        .iter()
        .find(|case| case.id == "nested-close-order-collapses-empty-branches")
        .unwrap();
    assert_eq!(accepted.disposition, "accept");
    assert!(accepted.initial.has_unique_ids_and_valid_ratios());
    let encoded = serde_json::to_vec(&accepted.initial).unwrap();
    assert_eq!(
        serde_json::from_slice::<SplitNode>(&encoded).unwrap(),
        accepted.initial
    );
    let mut root = Some(accepted.initial.clone());
    for (index, raw_id) in accepted.close_order.iter().enumerate() {
        root = root
            .as_ref()
            .and_then(|node| node.removing_pane(PaneId::from_str(raw_id).unwrap()));
        assert_eq!(root, accepted.expected_after_close[index]);
    }

    let rejected = corpus
        .cases
        .iter()
        .find(|case| case.id == "reject-duplicate-pane-id")
        .unwrap();
    assert_eq!(rejected.disposition, "reject");
    assert!(!rejected.initial.has_unique_ids_and_valid_ratios());
}

#[test]
fn tab_focus_cycle_split_swap_and_surface_selection_are_pure_model_operations() {
    let a = PaneId::new();
    let b = PaneId::new();
    let (pane_a, _) = terminal_pane(a);
    let (pane_b, _) = browser_pane(b);
    let mut tab = TabModel::single(TabId::new(), pane_a);

    tab.split_pane(a, SplitAxis::LeftRight, pane_b).unwrap();
    assert_eq!(tab.focused_pane_id(), b);
    assert!(tab.cycle_focus(1));
    assert_eq!(tab.focused_pane_id(), a);
    assert!(tab.cycle_focus(-1));
    assert_eq!(tab.focused_pane_id(), b);
    assert!(tab.swap_panes(a, b));
    assert_eq!(tab.pane_ids(), vec![b, a]);

    let (extra_runtime, _) = MockTerminalRuntime::new();
    let extra_id = SurfaceId::new();
    tab.add_surface(
        b,
        PaneSurface::new(extra_id, PaneRuntime::Terminal(Box::new(extra_runtime))),
    )
    .unwrap();
    assert_eq!(tab.pane(b).unwrap().active_surface().id(), extra_id);
    let original_surface = tab.pane(b).unwrap().surfaces()[0].id();
    assert!(tab.select_surface(b, original_surface));
    assert_eq!(tab.pane(b).unwrap().active_surface().id(), original_surface);
}

#[test]
fn hierarchy_reorder_keeps_the_selected_object_active() {
    let pane_a = PaneId::new();
    let pane_b = PaneId::new();
    let (tab_a, _) = tab_with_terminal(TabId::new(), pane_a);
    let tab_a_id = tab_a.id();
    let (tab_b, _) = tab_with_terminal(TabId::new(), pane_b);
    let tab_b_id = tab_b.id();
    let mut group = GroupModel::new(GroupId::new(), vec![tab_a, tab_b], 1).unwrap();

    assert!(group.move_tab(tab_b_id, 0));
    assert_eq!(group.active_tab().id(), tab_b_id);
    assert_eq!(group.tabs()[1].id(), tab_a_id);

    let group_a_id = group.id();
    let (group_b, _) = group_with_terminal(GroupId::new(), TabId::new(), PaneId::new());
    let group_b_id = group_b.id();
    let mut workspace = WorkspaceModel::new(WorkspaceId::new(), vec![group, group_b], 1).unwrap();
    assert!(workspace.move_group(group_b_id, 0));
    assert_eq!(workspace.active_group().id(), group_b_id);
    assert_eq!(workspace.groups()[1].id(), group_a_id);

    let workspace_a_id = workspace.id();
    let (workspace_b, _) = workspace_with_terminal(
        WorkspaceId::new(),
        GroupId::new(),
        TabId::new(),
        PaneId::new(),
    );
    let workspace_b_id = workspace_b.id();
    let mut app = AppModel::new(vec![workspace, workspace_b], 1).unwrap();
    assert!(app.move_workspace(workspace_b_id, 0));
    assert_eq!(app.active_workspace().id(), workspace_b_id);
    assert_eq!(app.workspaces()[1].id(), workspace_a_id);
}

#[test]
fn hierarchy_names_and_explicit_closes_preserve_ids_and_non_empty_parents() {
    let pane_a = PaneId::new();
    let pane_b = PaneId::new();
    let (mut tab_a, probe_a) = tab_with_terminal(TabId::new(), pane_a);
    let tab_a_id = tab_a.id();
    assert!(tab_a.rename(Some("  logs  ")));
    assert_eq!(tab_a.custom_title(), Some("logs"));
    assert!(tab_a.rename(Some("  ")));
    assert_eq!(tab_a.custom_title(), None);

    let (tab_b, _) = tab_with_terminal(TabId::new(), pane_b);
    let mut group = GroupModel::new(GroupId::new(), vec![tab_a, tab_b], 0).unwrap();
    let group_id = group.id();
    assert!(group.rename("  workers  "));
    assert_eq!(group.name(), "workers");
    assert!(!group.rename("\n\t"));
    assert_eq!(group.close_tab(0), Some(tab_a_id));
    assert!(probe_a.snapshot().closed);
    assert_eq!(group.close_tab(0), None);

    let (group_b, probe_b) = group_with_terminal(GroupId::new(), TabId::new(), PaneId::new());
    let mut workspace = WorkspaceModel::new(WorkspaceId::new(), vec![group, group_b], 0).unwrap();
    let workspace_id = workspace.id();
    assert!(workspace.rename("  deploy  "));
    assert_eq!(workspace.name(), "deploy");
    assert_eq!(workspace.close_group(0), Some(group_id));
    assert_eq!(workspace.close_group(0), None);
    assert!(!probe_b.snapshot().closed);

    let (workspace_b, probe_c) = workspace_with_terminal(
        WorkspaceId::new(),
        GroupId::new(),
        TabId::new(),
        PaneId::new(),
    );
    let mut app = AppModel::new(vec![workspace, workspace_b], 0).unwrap();
    assert!(app.cycle_workspace(1));
    assert!(app.cycle_workspace(-1));
    assert_eq!(app.close_workspace(0), Some(workspace_id));
    assert!(probe_b.snapshot().closed);
    assert_eq!(app.close_workspace(0), None);
    assert!(!probe_c.snapshot().closed);
}

#[test]
fn rename_by_id_does_not_follow_the_active_selection() {
    let (workspace_a, _) = workspace_with_terminal(
        WorkspaceId::new(),
        GroupId::new(),
        TabId::new(),
        PaneId::new(),
    );
    let (workspace_b, _) = workspace_with_terminal(
        WorkspaceId::new(),
        GroupId::new(),
        TabId::new(),
        PaneId::new(),
    );
    let workspace_b_id = workspace_b.id();
    let group_b_id = workspace_b.groups()[0].id();
    let tab_b_id = workspace_b.groups()[0].tabs()[0].id();
    let mut app = AppModel::new(vec![workspace_a, workspace_b], 0).unwrap();

    assert!(app.rename_workspace(workspace_b_id, "Second"));
    assert!(app.rename_group(group_b_id, "Remote"));
    assert!(app.rename_tab(tab_b_id, Some("Logs")));
    assert_eq!(app.active_workspace().name(), "Workspace 1");
    assert_eq!(app.workspaces()[1].name(), "Second");
    assert_eq!(app.workspaces()[1].groups()[0].name(), "Remote");
    assert_eq!(
        app.workspaces()[1].groups()[0].tabs()[0].custom_title(),
        Some("Logs")
    );
}

#[test]
fn focusing_hidden_pane_selects_its_full_hierarchy() {
    let hidden_pane = PaneId::new();
    let (hidden_workspace, _) = workspace_with_terminal(
        WorkspaceId::new(),
        GroupId::new(),
        TabId::new(),
        hidden_pane,
    );
    let (visible_workspace, _) = workspace_with_terminal(
        WorkspaceId::new(),
        GroupId::new(),
        TabId::new(),
        PaneId::new(),
    );
    let mut app = AppModel::new(vec![hidden_workspace, visible_workspace], 1).unwrap();

    assert!(app.focus_pane(hidden_pane));
    assert_eq!(app.active_workspace_index(), 0);
    assert_eq!(app.active_tab().focused_pane_id(), hidden_pane);
    assert!(!app.focus_pane(PaneId::new()));
}

#[test]
fn closing_one_pane_collapses_tree_closes_all_surfaces_and_selects_neighbor() {
    let a = PaneId::new();
    let b = PaneId::new();
    let (terminal, terminal_probe) = MockTerminalRuntime::new();
    let (browser, browser_probe) = MockBrowserRuntime::new();
    let pane_a = PaneContainer::new(
        a,
        vec![
            PaneSurface::with_new_id(PaneRuntime::Terminal(Box::new(terminal))),
            PaneSurface::with_new_id(PaneRuntime::Browser(Box::new(browser))),
        ],
        1,
    )
    .unwrap();
    let (pane_b, remaining_probe) = terminal_pane(b);
    let root = SplitNode::Split(Split::new(
        SplitAxis::LeftRight,
        0.5,
        SplitNode::pane(a),
        SplitNode::pane(b),
    ));
    let tab = TabModel::new(TabId::new(), root, a, vec![pane_a, pane_b]).unwrap();
    let app_group = GroupModel::single(GroupId::new(), tab);
    let workspace = WorkspaceModel::single(WorkspaceId::new(), app_group);
    let mut app = AppModel::single(workspace);

    assert_eq!(
        app.close_pane(a),
        Some(CloseOutcome::Removed(CloseScope::Pane(a)))
    );
    assert_eq!(app.active_tab().pane_ids(), vec![b]);
    assert_eq!(app.active_tab().focused_pane_id(), b);
    assert!(terminal_probe.snapshot().closed);
    assert!(browser_probe.snapshot().closed);
    assert!(!remaining_probe.snapshot().closed);
}

#[test]
fn close_chain_removes_tab_then_group_then_workspace() {
    let pane_a = PaneId::new();
    let pane_b = PaneId::new();
    let (tab_a, probe_a) = tab_with_terminal(TabId::new(), pane_a);
    let tab_a_id = tab_a.id();
    let (tab_b, _) = tab_with_terminal(TabId::new(), pane_b);
    let group_a = GroupModel::new(GroupId::new(), vec![tab_a, tab_b], 0).unwrap();
    let workspace_a = WorkspaceModel::single(WorkspaceId::new(), group_a);
    let mut app = AppModel::single(workspace_a);

    assert_eq!(
        app.close_pane(pane_a),
        Some(CloseOutcome::Removed(CloseScope::Tab(tab_a_id)))
    );
    assert!(probe_a.snapshot().closed);
    assert_eq!(app.active_workspace().active_group().tabs().len(), 1);

    let pane_c = PaneId::new();
    let (group_c, probe_c) = group_with_terminal(GroupId::new(), TabId::new(), pane_c);
    let group_c_id = group_c.id();
    app.active_workspace_mut().append_group(group_c).unwrap();
    assert_eq!(
        app.close_pane(pane_c),
        Some(CloseOutcome::Removed(CloseScope::Group(group_c_id)))
    );
    assert!(probe_c.snapshot().closed);

    let pane_d = PaneId::new();
    let (workspace_d, probe_d) =
        workspace_with_terminal(WorkspaceId::new(), GroupId::new(), TabId::new(), pane_d);
    let workspace_d_id = workspace_d.id();
    app.append_workspace(workspace_d).unwrap();
    assert_eq!(
        app.close_pane(pane_d),
        Some(CloseOutcome::Removed(CloseScope::Workspace(workspace_d_id)))
    );
    assert!(probe_d.snapshot().closed);
}

#[test]
fn final_workspace_requires_host_close_without_destroying_live_runtime() {
    let pane_id = PaneId::new();
    let (workspace, probe) =
        workspace_with_terminal(WorkspaceId::new(), GroupId::new(), TabId::new(), pane_id);
    let workspace_id = workspace.id();
    let mut app = AppModel::single(workspace);

    assert_eq!(
        app.close_pane(pane_id),
        Some(CloseOutcome::HostCloseRequired(CloseScope::Workspace(
            workspace_id
        )))
    );
    assert_eq!(app.workspaces().len(), 1);
    assert!(!probe.snapshot().closed);
}

#[test]
fn hidden_and_inactive_terminal_surfaces_keep_pumping_without_visibility_or_input() {
    let visible_pane_id = PaneId::new();
    let hidden_pane_id = PaneId::new();
    let (inactive_terminal, inactive_probe) = MockTerminalRuntime::new();
    let (active_browser, browser_probe) = MockBrowserRuntime::new();
    let visible_pane = PaneContainer::new(
        visible_pane_id,
        vec![
            PaneSurface::with_new_id(PaneRuntime::Terminal(Box::new(inactive_terminal))),
            PaneSurface::with_new_id(PaneRuntime::Browser(Box::new(active_browser))),
        ],
        1,
    )
    .unwrap();
    let visible_tab = TabModel::single(TabId::new(), visible_pane);
    let (hidden_tab, hidden_probe) = tab_with_terminal(TabId::new(), hidden_pane_id);
    let group = GroupModel::new(GroupId::new(), vec![visible_tab, hidden_tab], 0).unwrap();
    let workspace = WorkspaceModel::single(WorkspaceId::new(), group);
    let mut app = AppModel::single(workspace);

    let presentations = app.runtime_presentations();
    assert_eq!(presentations.len(), 3);
    let active_browser_state = presentations
        .iter()
        .find(|state| state.kind == RuntimeKind::Browser)
        .unwrap();
    assert!(active_browser_state.layout_visible);
    assert!(active_browser_state.surface_visible);
    assert!(active_browser_state.accepts_input);
    assert!(!active_browser_state.pumps_while_hidden);

    let terminal_states: Vec<_> = presentations
        .iter()
        .filter(|state| state.kind == RuntimeKind::Terminal)
        .collect();
    assert_eq!(terminal_states.len(), 2);
    assert!(terminal_states.iter().all(|state| !state.surface_visible));
    assert!(terminal_states.iter().all(|state| !state.accepts_input));
    assert!(terminal_states.iter().all(|state| state.pumps_while_hidden));

    app.pump_terminals();
    assert_eq!(inactive_probe.snapshot().pump_count, 1);
    assert_eq!(hidden_probe.snapshot().pump_count, 1);
    assert_eq!(browser_probe.snapshot().pump_count, 0);
}

#[test]
fn global_duplicate_pane_and_surface_ids_are_rejected() {
    let duplicate_pane = PaneId::new();
    let duplicate_surface = SurfaceId::new();
    let (runtime_a, _) = MockTerminalRuntime::new();
    let (runtime_b, _) = MockTerminalRuntime::new();
    let pane_a = PaneContainer::new(
        duplicate_pane,
        vec![PaneSurface::new(
            duplicate_surface,
            PaneRuntime::Terminal(Box::new(runtime_a)),
        )],
        0,
    )
    .unwrap();
    let pane_b = PaneContainer::new(
        duplicate_pane,
        vec![PaneSurface::new(
            SurfaceId::new(),
            PaneRuntime::Terminal(Box::new(runtime_b)),
        )],
        0,
    )
    .unwrap();
    let workspace_a = WorkspaceModel::single(
        WorkspaceId::new(),
        GroupModel::single(GroupId::new(), TabModel::single(TabId::new(), pane_a)),
    );
    let workspace_b = WorkspaceModel::single(
        WorkspaceId::new(),
        GroupModel::single(GroupId::new(), TabModel::single(TabId::new(), pane_b)),
    );
    assert!(matches!(
        AppModel::new(vec![workspace_a, workspace_b], 0),
        Err(ModelError::DuplicateId("pane"))
    ));

    let pane_c_id = PaneId::new();
    let pane_d_id = PaneId::new();
    let (runtime_c, _) = MockTerminalRuntime::new();
    let (runtime_d, _) = MockTerminalRuntime::new();
    let pane_c = PaneContainer::new(
        pane_c_id,
        vec![PaneSurface::new(
            duplicate_surface,
            PaneRuntime::Terminal(Box::new(runtime_c)),
        )],
        0,
    )
    .unwrap();
    let pane_d = PaneContainer::new(
        pane_d_id,
        vec![PaneSurface::new(
            duplicate_surface,
            PaneRuntime::Terminal(Box::new(runtime_d)),
        )],
        0,
    )
    .unwrap();
    let app = AppModel::new(
        vec![
            WorkspaceModel::single(
                WorkspaceId::new(),
                GroupModel::single(GroupId::new(), TabModel::single(TabId::new(), pane_c)),
            ),
            WorkspaceModel::single(
                WorkspaceId::new(),
                GroupModel::single(GroupId::new(), TabModel::single(TabId::new(), pane_d)),
            ),
        ],
        0,
    );
    assert!(matches!(app, Err(ModelError::DuplicateId("surface"))));
}

#[test]
fn tab_constructor_rejects_tree_registry_mismatch_and_invalid_focus() {
    let a = PaneId::new();
    let b = PaneId::new();
    let (pane_a, _) = terminal_pane(a);
    assert!(matches!(
        TabModel::new(TabId::new(), SplitNode::pane(b), b, vec![pane_a]),
        Err(ModelError::PaneRegistryMismatch)
    ));

    let (pane_a, _) = terminal_pane(a);
    assert!(matches!(
        TabModel::new(TabId::new(), SplitNode::pane(a), b, vec![pane_a]),
        Err(ModelError::UnknownPane(id)) if id == b
    ));
}
