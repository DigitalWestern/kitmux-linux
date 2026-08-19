use kitmux_model::{
    AppSnapshot, LoadDisposition, PaneDetail, PaneId, SplitNode, TabGroupSnapshot,
    TerminalTabSnapshot, WorkspaceId, WorkspaceSnapshot, decode_settings, encode_snapshot,
    last_good_path, load_settings_at_launch, load_state_at_launch, save_settings, save_state,
};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kitmux-persistence-{}-{nonce}-{}",
            std::process::id(),
            TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn snapshot(cwd: &str, resume_command: Option<&str>) -> AppSnapshot {
    let pane = PaneId::new();
    AppSnapshot {
        version: 1,
        active_workspace_index: 0,
        created_workspace_count: 1,
        workspaces: vec![WorkspaceSnapshot {
            id: Some(WorkspaceId::new()),
            name: "Workspace 1".to_owned(),
            active_tab_group_index: 0,
            created_group_count: 1,
            tab_groups: vec![TabGroupSnapshot {
                name: "Group 1".to_owned(),
                active_terminal_tab_index: 0,
                terminal_tabs: vec![TerminalTabSnapshot {
                    focused_pane_id: pane,
                    root: SplitNode::pane(pane),
                    custom_title: None,
                    pane_details: Some(BTreeMap::from([(
                        pane.to_string(),
                        PaneDetail {
                            cwd: Some(cwd.to_owned()),
                            resume_command: resume_command.map(str::to_owned),
                            ..PaneDetail::default()
                        },
                    )])),
                }],
            }],
            color_index: None,
        }],
        font_size: Some(18.0),
    }
}

#[test]
fn settings_launch_policy_handles_missing_corrupt_newer_and_unknown_fields() {
    let temp = TestDirectory::new();
    let path = temp.path().join("settings.json");
    let missing = load_settings_at_launch(&path);
    assert_eq!(missing.disposition, LoadDisposition::Missing);
    assert!(missing.may_write);

    fs::write(&path, b"not-json").unwrap();
    let corrupt = load_settings_at_launch(&path);
    assert!(matches!(corrupt.disposition, LoadDisposition::SetAside(_)));
    assert!(!path.exists());

    fs::write(&path, br#"{"version":99}"#).unwrap();
    let newer = load_settings_at_launch(&path);
    assert!(matches!(newer.disposition, LoadDisposition::SetAside(_)));
    assert!(!path.exists());

    fs::write(&path, br#"{"version":1,"future":{"kept":true}}"#).unwrap();
    let loaded = load_settings_at_launch(&path);
    save_settings(&path, &loaded.document).unwrap();
    assert_eq!(
        decode_settings(&fs::read(&path).unwrap()).unwrap().raw()["future"]["kept"],
        true
    );
}

#[test]
fn state_recovers_last_good_and_never_executes_inert_resume_text() {
    let temp = TestDirectory::new();
    let path = temp.path().join("state.json");
    save_state(&path, snapshot("/tmp", Some("touch /tmp/must-not-run"))).unwrap();
    fs::write(&path, b"broken").unwrap();

    let loaded = load_state_at_launch(&path);
    assert_eq!(loaded.disposition, LoadDisposition::RecoveredFromLastGood);
    let restored = loaded.snapshot.unwrap();
    let detail = restored.workspaces[0].tab_groups[0].terminal_tabs[0]
        .pane_details
        .as_ref()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(detail.cwd.as_deref(), Some("/tmp"));
    assert_eq!(
        detail.resume_command.as_deref(),
        Some("touch /tmp/must-not-run")
    );
    assert!(!Path::new("/tmp/must-not-run").exists());
    assert!(last_good_path(&path).exists());
}

#[test]
fn save_state_replaces_a_corrupt_primary_without_losing_the_last_good_backup() {
    let temp = TestDirectory::new();
    let path = temp.path().join("state.json");
    save_state(&path, snapshot("/tmp", None)).unwrap();
    fs::write(&path, b"broken").unwrap();

    save_state(&path, snapshot("/", None)).unwrap();

    let loaded = load_state_at_launch(&path);
    assert_eq!(loaded.disposition, LoadDisposition::Loaded);
    let restored = loaded.snapshot.unwrap();
    let detail = restored.workspaces[0].tab_groups[0].terminal_tabs[0]
        .pane_details
        .as_ref()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(detail.cwd.as_deref(), Some("/"));

    let backup = load_state_at_launch(&last_good_path(&path));
    let backup = backup.snapshot.unwrap();
    let detail = backup.workspaces[0].tab_groups[0].terminal_tabs[0]
        .pane_details
        .as_ref()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(detail.cwd.as_deref(), Some("/tmp"));
}

#[test]
fn failed_private_write_preserves_the_last_readable_files() {
    let temp = TestDirectory::new();
    let path = temp.path().join("state.json");
    let original = encode_snapshot(snapshot("/tmp", None)).unwrap();
    fs::write(&path, &original).unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o500)).unwrap();
    assert!(save_state(&path, snapshot("/", None)).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn failed_last_good_write_never_commits_the_new_primary() {
    let temp = TestDirectory::new();
    let path = temp.path().join("state.json");
    save_state(&path, snapshot("/tmp", None)).unwrap();
    let original = fs::read(&path).unwrap();
    let last_good = last_good_path(&path);
    fs::remove_file(&last_good).unwrap();
    symlink(temp.path().join("missing-target"), &last_good).unwrap();

    assert!(save_state(&path, snapshot("/", None)).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
}
