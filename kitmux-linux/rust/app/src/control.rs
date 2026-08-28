use gtk::glib::{self};
use kitmux_model::{
    CONTROL_DISPATCH_TIMEOUT, CommandId, ControlEventHistory, ControlMethod, ControlRequest,
    ControlResponse, ControlServer, ControlSocketError, PaneId, SshProfileStoreError, SurfaceId,
    encode_control_response, paste_confirmation_reason, resolve_control_socket,
};
use serde_json::json;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::diagnostic;
use crate::dialogs::paste_reason;
use crate::ffi;
use crate::navigation::{ForegroundScope, NavigationEffect, apply_navigation_effect};
use crate::runtime::owned_c_string;
use crate::ssh::{resolve_ssh_profile, ssh_review_json};
use crate::terminal::{Terminal, attach_missing_pty_sources};

thread_local! {
    pub(crate) static CONTROL_WAKE: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}
pub(crate) const IMPLEMENTED_CONTROL_METHODS: &[ControlMethod] = &[
    ControlMethod::Ping,
    ControlMethod::Tree,
    ControlMethod::Identify,
    ControlMethod::Capabilities,
    ControlMethod::EventList,
    ControlMethod::WorkspaceCreate,
    ControlMethod::WorkspaceSelect,
    ControlMethod::WorkspaceRename,
    ControlMethod::WorkspaceMove,
    ControlMethod::WorkspaceClose,
    ControlMethod::GroupCreate,
    ControlMethod::GroupSelect,
    ControlMethod::GroupRename,
    ControlMethod::GroupMove,
    ControlMethod::GroupClose,
    ControlMethod::TabCreate,
    ControlMethod::TabSelect,
    ControlMethod::TabRename,
    ControlMethod::TabMove,
    ControlMethod::TabClose,
    ControlMethod::PaneSplit,
    ControlMethod::PaneFocus,
    ControlMethod::PaneMove,
    ControlMethod::PaneClose,
    ControlMethod::PaneSend,
    ControlMethod::PaneSendKey,
    ControlMethod::PaneReadScreen,
    ControlMethod::PaneNotify,
    ControlMethod::SshProfileList,
    ControlMethod::SshConnect,
    ControlMethod::SshReconnect,
];

pub(crate) struct PendingControlCall {
    pub(crate) request: ControlRequest,
    pub(crate) peer_uid: u32,
    pub(crate) response: SyncSender<ControlResponse>,
}

pub(crate) fn install_control_server(
    terminal: &Rc<RefCell<Terminal>>,
) -> Result<(), ControlSocketError> {
    let environment: HashMap<String, String> = env::vars().collect();
    let address = resolve_control_socket(&environment, unsafe { libc::geteuid() })
        .map_err(|error| ControlSocketError::Path(error.to_string()))?;
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let wake_context = glib::MainContext::default();
    let wake_pending = Arc::new(AtomicBool::new(false));
    let history = terminal.borrow().control_history.clone();
    let handler_queue = Arc::clone(&queue);
    let handler_history = history.clone();
    let handler_wake_context = wake_context.clone();
    let handler_wake_pending = Arc::clone(&wake_pending);
    let server = ControlServer::start(address.clone(), history.clone(), move |request, peer| {
        let (sender, receiver) = mpsc::sync_channel(1);
        let request_id = request.id.clone();
        let request_method = request.method.clone();
        let mut queue = handler_queue
            .lock()
            .expect("control dispatch queue lock poisoned");
        if queue.len() >= 128 {
            handler_history.record(&request_method, &request_id, false, peer.uid);
            return ControlResponse::failure(request_id, "busy", "control dispatch queue is full");
        }
        queue.push_back(PendingControlCall {
            request,
            peer_uid: peer.uid,
            response: sender,
        });
        drop(queue);
        if !handler_wake_pending.swap(true, Ordering::AcqRel) {
            handler_wake_context.invoke(|| {
                CONTROL_WAKE.with(|wake| {
                    if let Some(dispatch) = wake.borrow().as_ref() {
                        dispatch();
                    }
                });
            });
        }
        match receiver.recv_timeout(CONTROL_DISPATCH_TIMEOUT) {
            Ok(response) => response,
            Err(_) => {
                handler_history.record(&request_method, &request_id, false, peer.uid);
                ControlResponse::failure(request_id, "timeout", "control request timed out")
            }
        }
    })?;
    let weak = Rc::downgrade(terminal);
    let dispatch_queue = Arc::clone(&queue);
    let dispatch_history = history.clone();
    let dispatch_pending = Arc::clone(&wake_pending);
    CONTROL_WAKE.with(|wake| {
        *wake.borrow_mut() = Some(Box::new(move || {
            dispatch_pending.store(false, Ordering::Release);
            let Some(terminal) = weak.upgrade() else {
                return;
            };
            let calls = dispatch_queue
                .lock()
                .expect("control dispatch queue lock poisoned")
                .drain(..)
                .collect::<Vec<_>>();
            for call in calls {
                let method = call.request.method.clone();
                let response = dispatch_control_request(&terminal, call.request, &dispatch_history);
                dispatch_history.record(&method, &response.id, response.ok, call.peer_uid);
                let _ = call.response.send(response);
            }
        }));
    });
    let mut terminal = terminal.borrow_mut();
    terminal.control_server = Some(server);
    diagnostic(
        "control_server_ready",
        &[
            ("socket", address.path().display().to_string()),
            ("mode", "600".to_owned()),
        ],
    );
    Ok(())
}

pub(crate) fn control_success(
    request: &ControlRequest,
    result: serde_json::Value,
) -> ControlResponse {
    ControlResponse::success(request.id.clone(), result)
}

pub(crate) fn control_failure(
    request: &ControlRequest,
    code: &str,
    message: impl Into<String>,
) -> ControlResponse {
    ControlResponse::failure(request.id.clone(), code, message)
}

pub(crate) fn dispatch_control_request(
    terminal: &Rc<RefCell<Terminal>>,
    request: ControlRequest,
    history: &ControlEventHistory,
) -> ControlResponse {
    let Some(method) = request.method_id() else {
        return control_failure(
            &request,
            "unknown_method",
            "method is not in the control catalog",
        );
    };
    if terminal.borrow().modal_dialog_open
        && !matches!(
            method,
            ControlMethod::Ping
                | ControlMethod::Identify
                | ControlMethod::Capabilities
                | ControlMethod::Tree
                | ControlMethod::EventList
                | ControlMethod::PaneReadScreen
        )
    {
        return control_failure(
            &request,
            "busy",
            "control mutations are paused while a modal dialog is open",
        );
    }
    if !matches!(
        method,
        ControlMethod::Ping
            | ControlMethod::Identify
            | ControlMethod::Capabilities
            | ControlMethod::EventList
    ) && terminal.borrow().navigation.is_none()
    {
        return control_failure(&request, "not_ready", "Kitmux is still initializing");
    }
    match method {
        ControlMethod::Ping => control_success(&request, json!({"message": "pong"})),
        ControlMethod::Identify => control_success(
            &request,
            json!({
                "pid": std::process::id(),
                "uid": unsafe { libc::geteuid() },
                "version": env!("CARGO_PKG_VERSION")
            }),
        ),
        ControlMethod::Capabilities => control_success(
            &request,
            json!({
                "protocolVersion": 1,
                "methods": ControlMethod::ALL.iter().map(|method| method.as_str()).collect::<Vec<_>>(),
                "implemented": IMPLEMENTED_CONTROL_METHODS.iter().map(|method| method.as_str()).collect::<Vec<_>>()
            }),
        ),
        ControlMethod::Tree => {
            let snapshot = terminal.borrow().snapshot();
            match serde_json::to_value(snapshot) {
                Ok(value) => control_success(&request, value),
                Err(error) => control_failure(&request, "internal_error", error.to_string()),
            }
        }
        ControlMethod::EventList => {
            let after = parse_u64(&request, "after").unwrap_or(0);
            let limit = parse_usize(&request, "limit").unwrap_or(100);
            let category = request.params.get("category").map(String::as_str);
            let events = history.list(after, limit, category);
            let cursor = history.cursor();
            control_success(&request, json!({"events": events, "eventCursor": cursor}))
        }
        ControlMethod::WorkspaceCreate => {
            control_navigation(terminal, &request, CommandId::WorkspaceNew)
        }
        ControlMethod::GroupCreate => control_navigation(terminal, &request, CommandId::GroupNew),
        ControlMethod::TabCreate => {
            control_navigation(terminal, &request, CommandId::TerminalNewTab)
        }
        ControlMethod::WorkspaceSelect => control_select(terminal, &request, "workspace"),
        ControlMethod::GroupSelect => control_select(terminal, &request, "group"),
        ControlMethod::TabSelect => control_select(terminal, &request, "tab"),
        ControlMethod::WorkspaceRename => control_rename(terminal, &request, "workspace"),
        ControlMethod::GroupRename => control_rename(terminal, &request, "group"),
        ControlMethod::TabRename => control_rename(terminal, &request, "tab"),
        ControlMethod::WorkspaceMove => control_move(terminal, &request, "workspace"),
        ControlMethod::GroupMove => control_move(terminal, &request, "group"),
        ControlMethod::TabMove => control_move(terminal, &request, "tab"),
        ControlMethod::WorkspaceClose => {
            control_close(terminal, &request, "workspace", ForegroundScope::Workspace)
        }
        ControlMethod::GroupClose => {
            control_close(terminal, &request, "group", ForegroundScope::Group)
        }
        ControlMethod::TabClose => control_close(terminal, &request, "tab", ForegroundScope::Tab),
        ControlMethod::PaneSplit => {
            let axis = match request.params.get("axis").map(String::as_str) {
                Some("right") => CommandId::PaneSplitRight,
                Some("down") => CommandId::PaneSplitDown,
                _ => {
                    return control_failure(
                        &request,
                        "invalid_params",
                        "pane.split axis must be right or down",
                    );
                }
            };
            if let Some(id) = request.params.get("id")
                && !select_pane(terminal, id)
            {
                return control_failure(&request, "not_found", "pane was not found");
            }
            control_navigation(terminal, &request, axis)
        }
        ControlMethod::PaneFocus => {
            let Some(id) = request.params.get("id") else {
                return control_failure(&request, "invalid_params", "pane.focus requires id");
            };
            let changed = select_pane(terminal, id);
            if changed {
                apply_navigation_effect(terminal, NavigationEffect::Changed);
                control_success(&request, json!({"changed": true}))
            } else {
                control_failure(&request, "not_found", "pane was not found")
            }
        }
        ControlMethod::PaneRename => control_failure(
            &request,
            "unsupported_method",
            "pane names are not stored by the Linux model",
        ),
        ControlMethod::PaneMove => {
            let Some(target) = request.params.get("target") else {
                return control_failure(&request, "invalid_params", "pane.move requires target");
            };
            let current = request
                .params
                .get("id")
                .map(String::as_str)
                .unwrap_or("current");
            if !select_pane(terminal, current) {
                return control_failure(&request, "not_found", "pane was not found");
            }
            let Ok(target) = PaneId::from_str(target) else {
                return control_failure(&request, "invalid_params", "target must be a pane ID");
            };
            let changed = {
                let mut terminal = terminal.borrow_mut();
                let Some(navigation) = terminal.navigation.as_mut() else {
                    return control_failure(&request, "not_ready", "navigation is not ready");
                };
                let current = navigation.active_tab().focused_pane_id();
                navigation.active_tab_mut().swap_panes(current, target)
            };
            if changed {
                apply_navigation_effect(terminal, NavigationEffect::Changed);
                control_success(&request, json!({"changed": true}))
            } else {
                control_failure(&request, "not_found", "target pane was not found")
            }
        }
        ControlMethod::PaneClose => {
            control_close(terminal, &request, "pane", ForegroundScope::Pane)
        }
        ControlMethod::PaneSend => control_send(terminal, &request),
        ControlMethod::PaneSendKey => control_send_key(terminal, &request),
        ControlMethod::PaneReadScreen => control_read_screen(terminal, &request),
        ControlMethod::PaneNotify => {
            let message = request.params.get("message").cloned().unwrap_or_default();
            if message.is_empty() {
                control_failure(&request, "invalid_params", "pane.notify requires message")
            } else {
                diagnostic("control_notify", &[("bytes", message.len().to_string())]);
                control_success(&request, json!({"message": "notification accepted"}))
            }
        }
        ControlMethod::SshProfileList => control_ssh_profile_list(terminal, &request),
        ControlMethod::SshConnect => control_ssh_connect(terminal, &request),
        ControlMethod::SshReconnect => control_ssh_reconnect(terminal, &request),
        _ => control_failure(
            &request,
            "unsupported_method",
            "method is reserved for a later Phase 6 slice",
        ),
    }
}

pub(crate) fn ssh_uuid_param(request: &ControlRequest, key: &str) -> Option<Uuid> {
    Uuid::parse_str(request.params.get(key)?).ok()
}

pub(crate) fn valid_ssh_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(crate) fn control_ssh_profile_list(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
) -> ControlResponse {
    if !request.params.is_empty() {
        return control_failure(
            request,
            "invalid_params",
            "ssh.profile.list takes no parameters",
        );
    }
    let profiles = terminal
        .borrow()
        .ssh_profiles
        .as_ref()
        .map(|store| {
            store
                .list()
                .into_iter()
                .map(|profile| {
                    json!({
                        "id": profile.id.to_string(),
                        "name": profile.name,
                        "hostAlias": profile.host_alias,
                        "hasRemoteCommand": profile.remote_command.is_some(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    control_success(request, json!({"sshProfiles": profiles}))
}

pub(crate) fn control_ssh_connect(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
) -> ControlResponse {
    control_ssh_action(terminal, request, false)
}

pub(crate) fn control_ssh_reconnect(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
) -> ControlResponse {
    control_ssh_action(terminal, request, true)
}

pub(crate) fn control_ssh_action(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    reconnect: bool,
) -> ControlResponse {
    let required = if reconnect { "pane" } else { "profile" };
    if request
        .params
        .keys()
        .any(|key| key != required && key != "fingerprint")
    {
        return control_failure(request, "invalid_params", "unknown SSH parameter");
    }
    let target = match ssh_uuid_param(request, required) {
        Some(value) => value,
        None => {
            return control_failure(
                request,
                "invalid_params",
                "SSH actions require an exact UUID",
            );
        }
    };
    let surface = if reconnect {
        let Some(surface) = resolve_pane_surface(terminal, &target.to_string()) else {
            return control_failure(request, "not_found", "SSH pane was not found");
        };
        let terminal_state = terminal.borrow();
        let Some(session) = terminal_state.sessions.get(&surface) else {
            return control_failure(request, "not_found", "SSH pane was not found");
        };
        if session.ssh_profile_id.is_none() {
            return control_failure(request, "invalid_params", "pane is not an SSH surface");
        }
        let disconnected = session
            .callback_ui
            .as_ref()
            .is_some_and(|callback| callback.disconnected.get());
        if !disconnected
            && !session.session.is_null()
            && unsafe { ffi::kitty_session_child_alive(session.session) }
        {
            return control_failure(request, "busy", "SSH pane is still connected");
        }
        Some(surface)
    } else {
        None
    };
    let profile_id = if let Some(surface) = surface {
        terminal
            .borrow()
            .sessions
            .get(&surface)
            .and_then(|session| session.ssh_profile_id)
            .expect("SSH surface has a profile ID")
    } else {
        target
    };
    let profile = terminal
        .borrow()
        .ssh_profiles
        .as_ref()
        .and_then(|store| store.profile(profile_id));
    let Some(profile) = profile else {
        return control_failure(request, "not_found", "SSH profile was not found");
    };
    let (executable, resolution) = match resolve_ssh_profile(&profile) {
        Ok(value) => value,
        Err(error) => return control_failure(request, "resolution_failed", error.to_string()),
    };
    let review = resolution.review(&profile);
    let requested_fingerprint = request.params.get("fingerprint");
    if let Some(fingerprint) = requested_fingerprint {
        if !valid_ssh_fingerprint(fingerprint) {
            return control_failure(
                request,
                "invalid_params",
                "fingerprint must be lowercase SHA-256",
            );
        }
        if fingerprint != &review.fingerprint {
            return control_failure(
                request,
                "stale_review",
                "SSH resolution changed; review the new fingerprint",
            );
        }
    }
    if review.requires_approval && requested_fingerprint != Some(&review.fingerprint) {
        return control_success(
            request,
            json!({
                "connected": false,
                "approvalRequired": true,
                "review": ssh_review_json(&review),
            }),
        );
    }
    if review.requires_approval {
        let approval = terminal
            .borrow()
            .ssh_profiles
            .as_ref()
            .ok_or(SshProfileStoreError::InvalidDocument)
            .and_then(|store| store.approve(profile.id, &review.fingerprint));
        if let Err(error) = approval {
            return control_failure(request, "store_failed", error.to_string());
        }
    }
    if let Some(surface) = surface {
        let replaced = terminal
            .borrow_mut()
            .replace_ssh_surface(surface, &profile, &executable);
        if !replaced {
            return control_failure(request, "launch_failed", "SSH reconnect could not start");
        }
        let _ = attach_missing_pty_sources(terminal);
        control_success(
            request,
            json!({
                "connected": true,
                "reconnected": true,
                "destination": review.destination,
                "pane": target.to_string(),
            }),
        )
    } else {
        let created = terminal.borrow_mut().create_ssh_tab(&profile, &executable);
        if !created {
            return control_failure(request, "launch_failed", "SSH tab could not start");
        }
        apply_navigation_effect(terminal, NavigationEffect::Changed);
        let pane = terminal
            .borrow()
            .navigation
            .as_ref()
            .map(|navigation| navigation.active_tab().focused_pane_id().to_string());
        control_success(
            request,
            json!({
                "connected": true,
                "destination": review.destination,
                "pane": pane,
            }),
        )
    }
}

pub(crate) fn control_navigation(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    command: CommandId,
) -> ControlResponse {
    let effect = terminal.borrow_mut().navigation_action(command);
    let accepted = matches!(
        effect,
        NavigationEffect::Changed | NavigationEffect::CloseWindow
    );
    if accepted {
        apply_navigation_effect(terminal, effect);
        control_success(request, json!({"changed": true}))
    } else {
        control_failure(request, "rejected", "navigation command was rejected")
    }
}

pub(crate) fn control_select(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    noun: &str,
) -> ControlResponse {
    let Some(id) = request.params.get("id") else {
        return control_failure(request, "invalid_params", "selection requires id");
    };
    let changed = match noun {
        "workspace" => select_workspace(terminal, id),
        "group" => select_group(terminal, id),
        "tab" => select_tab(terminal, id),
        _ => false,
    };
    if changed {
        apply_navigation_effect(terminal, NavigationEffect::Changed);
        control_success(request, json!({"changed": true}))
    } else {
        control_failure(request, "not_found", format!("{noun} was not found"))
    }
}

pub(crate) fn control_rename(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    noun: &str,
) -> ControlResponse {
    let Some(name) = request.params.get("name") else {
        return control_failure(request, "invalid_params", "rename requires name");
    };
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        return control_failure(
            request,
            "invalid_params",
            "name is empty or contains controls",
        );
    }
    let id = request
        .params
        .get("id")
        .map(String::as_str)
        .unwrap_or("current");
    let previous_selection = navigation_selection(terminal);
    if !control_target_exists(terminal, noun, id) {
        return control_failure(request, "not_found", format!("{noun} was not found"));
    }
    let selected = match noun {
        "workspace" => select_workspace(terminal, id),
        "group" => select_group(terminal, id),
        "tab" => select_tab(terminal, id),
        _ => false,
    };
    if !selected {
        return control_failure(request, "not_ready", "navigation is not ready");
    }
    let changed = {
        let mut terminal = terminal.borrow_mut();
        let Some(navigation) = terminal.navigation.as_mut() else {
            return control_failure(request, "not_ready", "navigation is not ready");
        };
        match noun {
            "workspace" => navigation.active_workspace_mut().rename(name),
            "group" => navigation
                .active_workspace_mut()
                .active_group_mut()
                .rename(name),
            "tab" => navigation.active_tab_mut().rename(Some(name)),
            _ => false,
        }
    };
    if changed {
        apply_navigation_effect(terminal, NavigationEffect::Changed);
        control_success(request, json!({"changed": true}))
    } else {
        restore_navigation(terminal, previous_selection);
        control_failure(request, "invalid_params", "name is unchanged")
    }
}

pub(crate) fn control_move(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    noun: &str,
) -> ControlResponse {
    let Some(id) = request.params.get("id") else {
        return control_failure(request, "invalid_params", "move requires id");
    };
    let Some(index) = request
        .params
        .get("index")
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return control_failure(request, "invalid_params", "move requires a numeric index");
    };
    let changed = {
        let mut terminal = terminal.borrow_mut();
        let Some(navigation) = terminal.navigation.as_mut() else {
            return control_failure(request, "not_ready", "navigation is not ready");
        };
        match noun {
            "workspace" => navigation
                .workspaces()
                .iter()
                .find(|item| item.id().to_string() == *id)
                .map(|item| item.id())
                .is_some_and(|id| navigation.move_workspace(id, index)),
            "group" => {
                let group = navigation
                    .active_workspace()
                    .groups()
                    .iter()
                    .find(|item| item.id().to_string() == *id)
                    .map(|item| item.id());
                group.is_some_and(|id| navigation.active_workspace_mut().move_group(id, index))
            }
            "tab" => {
                let tab = navigation
                    .active_workspace()
                    .active_group()
                    .tabs()
                    .iter()
                    .find(|item| item.id().to_string() == *id)
                    .map(|item| item.id());
                tab.is_some_and(|id| {
                    navigation
                        .active_workspace_mut()
                        .active_group_mut()
                        .move_tab(id, index)
                })
            }
            _ => false,
        }
    };
    if changed {
        apply_navigation_effect(terminal, NavigationEffect::Changed);
        control_success(request, json!({"changed": true}))
    } else {
        control_failure(request, "rejected", format!("{noun} move was rejected"))
    }
}

pub(crate) fn control_close(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    noun: &str,
    scope: ForegroundScope,
) -> ControlResponse {
    let id = request
        .params
        .get("id")
        .map(String::as_str)
        .unwrap_or("current");
    let previous_selection = navigation_selection(terminal);
    if !control_target_exists(terminal, noun, id) {
        return control_failure(request, "not_found", format!("{noun} was not found"));
    }
    let selected = match noun {
        "workspace" => select_workspace(terminal, id),
        "group" => select_group(terminal, id),
        "tab" => select_tab(terminal, id),
        "pane" => select_pane(terminal, id),
        _ => false,
    };
    if !selected {
        return control_failure(request, "not_ready", "navigation is not ready");
    }
    let force = request
        .params
        .get("force")
        .is_some_and(|value| value == "true");
    let previous_close_confirmed = terminal.borrow().close_confirmed;
    let foreground = terminal.borrow().foreground_surfaces(Some(scope));
    if !foreground.is_empty() && !force {
        restore_navigation(terminal, previous_selection);
        return control_failure(
            request,
            "confirmation_required",
            "a foreground process is running; retry with force=true",
        );
    }
    if force {
        terminal.borrow_mut().close_confirmed = true;
    }
    let mut applied_effect = false;
    let changed = match noun {
        "pane" => {
            let effect = terminal
                .borrow_mut()
                .navigation_action(CommandId::PaneClose);
            let changed = matches!(
                effect,
                NavigationEffect::Changed | NavigationEffect::CloseWindow
            );
            if changed {
                apply_navigation_effect(terminal, effect);
                applied_effect = true;
            }
            changed
        }
        "tab" => {
            let mut terminal = terminal.borrow_mut();
            let Some(navigation) = terminal.navigation.as_mut() else {
                terminal.close_confirmed = previous_close_confirmed;
                return control_failure(request, "not_ready", "navigation is not ready");
            };
            let index = navigation
                .active_workspace()
                .active_group()
                .active_tab_index();
            navigation
                .active_workspace_mut()
                .active_group_mut()
                .close_tab(index)
                .is_some()
        }
        "group" => {
            let mut terminal = terminal.borrow_mut();
            let Some(navigation) = terminal.navigation.as_mut() else {
                terminal.close_confirmed = previous_close_confirmed;
                return control_failure(request, "not_ready", "navigation is not ready");
            };
            let index = navigation.active_workspace().active_group_index();
            navigation
                .active_workspace_mut()
                .close_group(index)
                .is_some()
        }
        "workspace" => {
            let mut terminal = terminal.borrow_mut();
            let Some(navigation) = terminal.navigation.as_mut() else {
                terminal.close_confirmed = previous_close_confirmed;
                return control_failure(request, "not_ready", "navigation is not ready");
            };
            navigation
                .close_workspace(navigation.active_workspace_index())
                .is_some()
        }
        _ => false,
    };
    terminal.borrow_mut().close_confirmed = previous_close_confirmed;
    if changed && !applied_effect {
        apply_navigation_effect(terminal, NavigationEffect::Changed);
        control_success(request, json!({"changed": true}))
    } else {
        restore_navigation(terminal, previous_selection);
        control_failure(request, "rejected", format!("{noun} close was rejected"))
    }
}

pub(crate) fn control_send(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
) -> ControlResponse {
    let Some(text) = request.params.get("text") else {
        return control_failure(request, "invalid_params", "pane.send requires text");
    };
    let id = request
        .params
        .get("id")
        .map(String::as_str)
        .unwrap_or("current");
    if !select_pane(terminal, id) {
        return control_failure(request, "not_found", "pane was not found");
    }
    apply_navigation_effect(terminal, NavigationEffect::Changed);
    if !active_surface_matches_pane(terminal, id) {
        return control_failure(
            request,
            "not_ready",
            "target pane is not the reconciled active surface",
        );
    }
    let force = request
        .params
        .get("force")
        .is_some_and(|value| value == "true");
    if !force {
        let threshold = terminal.borrow().paste_confirmation_threshold;
        if let Some(reason) = paste_confirmation_reason(text, threshold) {
            return control_failure(
                request,
                "confirmation_required",
                format!("pane.send requires confirmation ({})", paste_reason(reason)),
            );
        }
    }
    terminal.borrow_mut().paste(text);
    control_success(request, json!({"byteCount": text.len()}))
}

pub(crate) fn control_send_key(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
) -> ControlResponse {
    let Some(key) = request.params.get("key") else {
        return control_failure(request, "invalid_params", "pane.send_key requires key");
    };
    let id = request
        .params
        .get("id")
        .map(String::as_str)
        .unwrap_or("current");
    if !select_pane(terminal, id) {
        return control_failure(request, "not_found", "pane was not found");
    }
    apply_navigation_effect(terminal, NavigationEffect::Changed);
    if !active_surface_matches_pane(terminal, id) {
        return control_failure(
            request,
            "not_ready",
            "target pane is not the reconciled active surface",
        );
    }
    let bytes = match key.as_str() {
        "Enter" => vec![b'\r'],
        "Tab" => vec![b'\t'],
        "Escape" => vec![0x1b],
        "Backspace" => vec![0x7f],
        value
            if value.chars().count() == 1
                && !value.chars().next().expect("one character").is_control() =>
        {
            value.as_bytes().to_vec()
        }
        _ => {
            return control_failure(
                request,
                "invalid_params",
                "key is not a supported safe keystroke",
            );
        }
    };
    let terminal = terminal.borrow();
    if terminal.session.is_null() {
        return control_failure(request, "not_ready", "terminal session is not ready");
    }
    unsafe { ffi::kitty_session_write(terminal.session, bytes.as_ptr().cast(), bytes.len()) };
    control_success(request, json!({"byteCount": bytes.len()}))
}

pub(crate) fn control_read_screen(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
) -> ControlResponse {
    let id = request
        .params
        .get("id")
        .map(String::as_str)
        .unwrap_or("current");
    let Some(surface) = resolve_pane_surface(terminal, id) else {
        return control_failure(request, "not_found", "pane was not found");
    };
    let lines = match request.params.get("lines") {
        Some(value) => match value.parse::<usize>() {
            Ok(lines) => Some(lines),
            Err(_) => {
                return control_failure(request, "invalid_params", "lines must be a number");
            }
        },
        None => None,
    };
    let terminal = terminal.borrow();
    let Some(session) = terminal.sessions.get(&surface) else {
        return control_failure(request, "not_ready", "terminal session is not ready");
    };
    if session.session.is_null() {
        return control_failure(request, "not_ready", "terminal session is not ready");
    }
    let Some(text) = owned_c_string(unsafe { ffi::kitty_session_text(session.session) }) else {
        return control_failure(
            request,
            "internal_error",
            "terminal screen text was unavailable",
        );
    };
    let total = text.len();
    let selected = lines.map_or_else(|| text.clone(), |count| tail_screen_lines(&text, count));
    let line_truncated = selected.len() != text.len();
    let mut low = 0;
    let mut high = selected.len();
    let mut bounded = String::new();
    while low <= high {
        let midpoint = low + (high - low) / 2;
        let candidate = utf8_prefix(&selected, midpoint);
        let response = screen_response(
            request,
            &candidate,
            total,
            line_truncated || candidate.len() != selected.len(),
        );
        if encode_control_response(&response).is_ok() {
            bounded = candidate;
            low = midpoint + 1;
        } else if midpoint == 0 {
            break;
        } else {
            high = midpoint - 1;
        }
    }
    let truncated = line_truncated || bounded.len() != selected.len();
    control_success(
        request,
        json!({
            "text": bounded,
            "byteCount": bounded.len(),
            "totalByteCount": total,
            "truncated": truncated
        }),
    )
}

pub(crate) fn screen_response(
    request: &ControlRequest,
    text: &str,
    total_byte_count: usize,
    truncated: bool,
) -> ControlResponse {
    control_success(
        request,
        json!({
            "text": text,
            "byteCount": text.len(),
            "totalByteCount": total_byte_count,
            "truncated": truncated
        }),
    )
}

pub(crate) fn tail_screen_lines(text: &str, count: usize) -> String {
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    let start = lines.len().saturating_sub(count);
    lines[start..].concat()
}

pub(crate) fn utf8_prefix(text: &str, maximum_bytes: usize) -> String {
    if maximum_bytes >= text.len() {
        return text.to_owned();
    }
    let end = text
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= maximum_bytes)
        .last()
        .unwrap_or(0);
    text[..end].to_owned()
}

pub(crate) fn parse_u64(request: &ControlRequest, key: &str) -> Option<u64> {
    request.params.get(key).and_then(|value| value.parse().ok())
}

pub(crate) fn parse_usize(request: &ControlRequest, key: &str) -> Option<usize> {
    request.params.get(key).and_then(|value| value.parse().ok())
}

pub(crate) fn select_workspace(terminal: &Rc<RefCell<Terminal>>, id: &str) -> bool {
    let mut terminal = terminal.borrow_mut();
    let Some(navigation) = terminal.navigation.as_mut() else {
        return false;
    };
    if id == "current" {
        return true;
    }
    let index = id.parse::<usize>().ok().or_else(|| {
        navigation
            .workspaces()
            .iter()
            .position(|item| item.id().to_string() == id)
    });
    index.is_some_and(|index| navigation.select_workspace(index))
}

pub(crate) fn select_group(terminal: &Rc<RefCell<Terminal>>, id: &str) -> bool {
    let mut terminal = terminal.borrow_mut();
    let Some(navigation) = terminal.navigation.as_mut() else {
        return false;
    };
    if id == "current" {
        return true;
    }
    let index = id.parse::<usize>().ok().or_else(|| {
        navigation
            .active_workspace()
            .groups()
            .iter()
            .position(|item| item.id().to_string() == id)
    });
    index.is_some_and(|index| navigation.active_workspace_mut().select_group(index))
}

pub(crate) fn select_tab(terminal: &Rc<RefCell<Terminal>>, id: &str) -> bool {
    let mut terminal = terminal.borrow_mut();
    let Some(navigation) = terminal.navigation.as_mut() else {
        return false;
    };
    if id == "current" {
        return true;
    }
    let index = id.parse::<usize>().ok().or_else(|| {
        navigation
            .active_workspace()
            .active_group()
            .tabs()
            .iter()
            .position(|item| item.id().to_string() == id)
    });
    index.is_some_and(|index| {
        navigation
            .active_workspace_mut()
            .active_group_mut()
            .select_tab(index)
    })
}

pub(crate) fn select_pane(terminal: &Rc<RefCell<Terminal>>, id: &str) -> bool {
    if id == "current" {
        return true;
    }
    let Ok(id) = PaneId::from_str(id) else {
        return false;
    };
    terminal
        .borrow_mut()
        .navigation
        .as_mut()
        .is_some_and(|navigation| navigation.focus_pane(id))
}

pub(crate) fn navigation_selection(
    terminal: &Rc<RefCell<Terminal>>,
) -> Option<(usize, usize, usize)> {
    let terminal = terminal.borrow();
    let navigation = terminal.navigation.as_ref()?;
    Some((
        navigation.active_workspace_index(),
        navigation.active_workspace().active_group_index(),
        navigation
            .active_workspace()
            .active_group()
            .active_tab_index(),
    ))
}

pub(crate) fn restore_navigation(
    terminal: &Rc<RefCell<Terminal>>,
    selection: Option<(usize, usize, usize)>,
) {
    let Some((workspace, group, tab)) = selection else {
        return;
    };
    let mut terminal = terminal.borrow_mut();
    let Some(navigation) = terminal.navigation.as_mut() else {
        return;
    };
    if navigation.select_workspace(workspace)
        && navigation.active_workspace_mut().select_group(group)
    {
        let _ = navigation
            .active_workspace_mut()
            .active_group_mut()
            .select_tab(tab);
    }
}

pub(crate) fn control_target_exists(
    terminal: &Rc<RefCell<Terminal>>,
    noun: &str,
    id: &str,
) -> bool {
    let terminal = terminal.borrow();
    let Some(navigation) = terminal.navigation.as_ref() else {
        return false;
    };
    if id == "current" {
        return true;
    }
    match noun {
        "workspace" => {
            id.parse::<usize>()
                .is_ok_and(|index| index < navigation.workspaces().len())
                || navigation
                    .workspaces()
                    .iter()
                    .any(|workspace| workspace.id().to_string() == id)
        }
        "group" => {
            id.parse::<usize>()
                .is_ok_and(|index| index < navigation.active_workspace().groups().len())
                || navigation
                    .active_workspace()
                    .groups()
                    .iter()
                    .any(|group| group.id().to_string() == id)
        }
        "tab" => {
            id.parse::<usize>().is_ok_and(|index| {
                index < navigation.active_workspace().active_group().tabs().len()
            }) || navigation
                .active_workspace()
                .active_group()
                .tabs()
                .iter()
                .any(|tab| tab.id().to_string() == id)
        }
        "pane" => PaneId::from_str(id).is_ok_and(|pane_id| {
            navigation
                .runtime_presentations()
                .iter()
                .any(|presentation| presentation.location.pane_id == pane_id)
        }),
        _ => false,
    }
}

pub(crate) fn resolve_pane_surface(
    terminal: &Rc<RefCell<Terminal>>,
    id: &str,
) -> Option<SurfaceId> {
    let terminal = terminal.borrow();
    if id == "current" {
        return terminal
            .sessions
            .contains_key(&terminal.active_surface_id)
            .then_some(terminal.active_surface_id);
    }
    let pane_id = PaneId::from_str(id).ok()?;
    let navigation = terminal.navigation.as_ref()?;
    navigation
        .runtime_presentations()
        .into_iter()
        .find_map(|presentation| {
            (presentation.location.pane_id == pane_id
                && terminal
                    .sessions
                    .contains_key(&presentation.location.surface_id))
            .then_some(presentation.location.surface_id)
        })
}

pub(crate) fn active_surface_matches_pane(terminal: &Rc<RefCell<Terminal>>, id: &str) -> bool {
    let active_surface = terminal.borrow().active_surface_id;
    resolve_pane_surface(terminal, id) == Some(active_surface)
}

#[cfg(test)]
mod control_surface_tests {
    use super::*;

    #[test]
    fn implemented_control_methods_are_catalogued() {
        assert_eq!(IMPLEMENTED_CONTROL_METHODS.len(), 31);
        assert!(
            IMPLEMENTED_CONTROL_METHODS
                .iter()
                .all(|method| ControlMethod::ALL.contains(method))
        );
        assert!(!IMPLEMENTED_CONTROL_METHODS.contains(&ControlMethod::PaneRename));
    }
}
