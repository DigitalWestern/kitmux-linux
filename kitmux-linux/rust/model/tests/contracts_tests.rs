use kitmux_model::{
    AtomicWriteError, CONTROL_MAX_REQUEST_BYTES, CONTROL_MAX_RESPONSE_BYTES, CliParseError,
    CommandId, ControlCodecError, ControlMethod, ControlResponse, FileChange, ImportPreviewError,
    LineFrameDecoder, PollingFileWatcher, RuntimePathError, SemanticAction, SettingsCodecError,
    SnapshotCodecError, SshCodecError, SshProfile, SshResolution, UnixSocketAddress, XdgPaths,
    atomic_write_private, decode_control_request, decode_control_response, decode_settings,
    decode_snapshot, decode_ssh_profiles, encode_control_response, encode_settings,
    encode_snapshot, encode_ssh_profiles, parse_cli, preview_macos_state_file, read_bounded,
    sha256_bytes, sha256_file, valid_resume_command,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use uuid::Uuid;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../contracts/fixtures/v1")
        .join(name);
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = PathBuf::from("/tmp").join(format!("km-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn frozen_state_snapshot_corpus_accepts_repairs_and_rejects() {
    let corpus = fixture("state-snapshots.json");
    for case in corpus["cases"].as_array().unwrap() {
        let bytes = serde_json::to_vec(&case["input"]).unwrap();
        match case["disposition"].as_str().unwrap() {
            "accept" => {
                let snapshot = decode_snapshot(&bytes).unwrap();
                assert_eq!(serde_json::to_value(&snapshot).unwrap(), case["input"]);
                let encoded = encode_snapshot(snapshot).unwrap();
                let encoded_value =
                    serde_json::to_value(decode_snapshot(&encoded).unwrap()).unwrap();
                assert_eq!(encoded_value, case["input"]);
                assert_eq!(
                    serde_json::to_vec(&encoded_value).unwrap(),
                    serde_json::to_vec(&case["input"]).unwrap()
                );
            }
            "repair" => {
                let snapshot = decode_snapshot(&bytes).unwrap();
                assert_eq!(serde_json::to_value(snapshot).unwrap(), case["expected"]);
            }
            "reject" => {
                assert!(matches!(
                    decode_snapshot(&bytes),
                    Err(SnapshotCodecError::Invalid("duplicate pane ID"))
                ));
            }
            other => panic!("unexpected fixture disposition {other}"),
        }
    }
}

#[test]
fn state_codec_rejects_newer_malformed_empty_and_oversized_documents() {
    assert_eq!(
        decode_snapshot(br#"{"version":2}"#),
        Err(SnapshotCodecError::UnsupportedVersion(2))
    );
    assert_eq!(decode_snapshot(b"{"), Err(SnapshotCodecError::Malformed));
    assert!(matches!(
        decode_snapshot(
            br#"{"version":1,"activeWorkspaceIndex":0,"createdWorkspaceCount":0,"workspaces":[]}"#
        ),
        Err(SnapshotCodecError::Invalid("empty workspace list"))
    ));
    assert_eq!(
        decode_snapshot(&vec![b' '; 8 * 1024 * 1024 + 1]),
        Err(SnapshotCodecError::TooLarge)
    );
}

#[test]
fn state_detail_repairs_unsafe_resume_paths_urls_and_surface_selection() {
    let pane = "11111111-1111-1111-1111-111111111111";
    let snapshot = json!({
        "version": 1,
        "activeWorkspaceIndex": 0,
        "createdWorkspaceCount": 1,
        "workspaces": [{
            "id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "name": "workspace 1",
            "activeTabGroupIndex": 0,
            "createdGroupCount": 1,
            "tabGroups": [{
                "name": "main",
                "activeTerminalTabIndex": 0,
                "terminalTabs": [{
                    "focusedPaneID": {"rawValue": pane},
                    "root": {"pane": {"_0": {"rawValue": pane}}},
                    "paneDetails": {
                        pane: {
                            "cwd": "/ignored",
                            "resumeCommand": "ignored",
                            "surfaces": [{
                                "id": "22222222-2222-2222-2222-222222222222",
                                "cwd": "relative",
                                "resumeCommand": "  make test  ",
                                "kind": "terminal"
                            }, {
                                "id": "33333333-3333-3333-3333-333333333333",
                                "cwd": "/ignored",
                                "resumeCommand": "ignored",
                                "kind": "browser",
                                "url": "https://"
                            }],
                            "activeSurfaceIndex": 99
                        }
                    }
                }]
            }]
        }]
    });
    let repaired = decode_snapshot(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let details = repaired.workspaces[0].tab_groups[0].terminal_tabs[0]
        .pane_details
        .as_ref()
        .unwrap()
        .get(pane)
        .unwrap();
    let surfaces = details.surfaces.as_ref().unwrap();
    assert_eq!(details.active_surface_index, Some(1));
    assert_eq!(surfaces[0].cwd, None);
    assert_eq!(surfaces[0].resume_command.as_deref(), Some("make test"));
    assert_eq!(surfaces[1].url, None);
    assert_eq!(details.cwd, None);
    assert_eq!(details.resume_command, None);
}

#[test]
fn state_surface_ids_are_deduplicated_per_pane_stack() {
    let first = "11111111-1111-1111-1111-111111111111";
    let second = "22222222-2222-2222-2222-222222222222";
    let shared_surface = "33333333-3333-3333-3333-333333333333";
    let snapshot = json!({
        "version": 1,
        "activeWorkspaceIndex": 0,
        "createdWorkspaceCount": 1,
        "workspaces": [{
            "id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "name": "workspace 1",
            "activeTabGroupIndex": 0,
            "createdGroupCount": 1,
            "tabGroups": [{
                "name": "main",
                "activeTerminalTabIndex": 0,
                "terminalTabs": [{
                    "focusedPaneID": {"rawValue": first},
                    "root": {"split": {"_0": {
                        "id": {"rawValue": "44444444-4444-4444-4444-444444444444"},
                        "axis": "leftRight",
                        "ratio": 0.5,
                        "first": {"pane": {"_0": {"rawValue": first}}},
                        "second": {"pane": {"_0": {"rawValue": second}}}
                    }}},
                    "paneDetails": {
                        first: {"surfaces": [{
                            "id": shared_surface,
                            "kind": "terminal"
                        }]},
                        second: {"surfaces": [{
                            "id": shared_surface,
                            "kind": "terminal"
                        }]}
                    }
                }]
            }]
        }]
    });
    let repaired = decode_snapshot(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let details = repaired.workspaces[0].tab_groups[0].terminal_tabs[0]
        .pane_details
        .as_ref()
        .unwrap();
    assert_eq!(details.len(), 2);
    assert!(
        details
            .values()
            .all(|detail| detail.surfaces.as_ref().unwrap().len() == 1)
    );
}

#[test]
fn resume_command_bound_is_utf8_bytes_and_rejects_controls() {
    assert_eq!(
        valid_resume_command(Some("  cargo test  ")).as_deref(),
        Some("cargo test")
    );
    assert_eq!(valid_resume_command(Some("cargo\ntest")), None);
    assert!(valid_resume_command(Some(&"é".repeat(1024))).is_some());
    assert_eq!(valid_resume_command(Some(&"é".repeat(1025))), None);
}

#[test]
fn frozen_settings_corpus_resolves_and_preserves_unknown_fields() {
    let corpus = fixture("settings.json");
    for case in corpus["cases"].as_array().unwrap() {
        let document = decode_settings(&serde_json::to_vec(&case["input"]).unwrap()).unwrap();
        if let Some(expected) = case.get("expectedValidated") {
            assert_eq!(Value::Object(document.validated_values()), *expected);
        }
        if let Some(expected) = case.get("expectedResolved") {
            assert_eq!(
                serde_json::to_value(document.resolved()).unwrap(),
                *expected
            );
        }
        let encoded: Value = serde_json::from_slice(&encode_settings(&document).unwrap()).unwrap();
        if case["input"].get("futureKey").is_some() {
            assert_eq!(encoded["futureKey"], true);
        }
    }
}

#[test]
fn settings_codec_bounds_versions_and_cross_field_ranges() {
    assert_eq!(
        decode_settings(br#"{"version":2}"#),
        Err(SettingsCodecError::UnsupportedVersion(2))
    );
    assert_eq!(decode_settings(b"[]"), Err(SettingsCodecError::Malformed));
    assert_eq!(
        decode_settings(&vec![b' '; 1024 * 1024 + 1]),
        Err(SettingsCodecError::TooLarge)
    );
    let document =
        decode_settings(br#"{"tabMinWidthPoints":200,"tabMaxWidthPoints":120}"#).unwrap();
    assert_eq!(document.resolved().tab_min_width_points, 90);
    assert_eq!(document.resolved().tab_max_width_points, 220);
}

#[test]
fn frozen_control_protocol_corpus_enforces_envelope_and_byte_bounds() {
    let corpus = fixture("control-protocol.json");
    for case in corpus["cases"].as_array().unwrap() {
        let data = if let Some(input) = case.get("input") {
            serde_json::to_vec(input).unwrap()
        } else if let Some(raw) = case.get("raw").and_then(Value::as_str) {
            raw.as_bytes().to_vec()
        } else {
            let generated = &case["generate"];
            if let Some(count) = generated.get("rawUtf8Bytes").and_then(Value::as_u64) {
                generated["scalar"]
                    .as_str()
                    .unwrap()
                    .repeat(count as usize)
                    .into_bytes()
            } else {
                let mut base = generated["base"].clone();
                base[generated["field"].as_str().unwrap()] = Value::String(
                    generated["scalar"]
                        .as_str()
                        .unwrap()
                        .repeat(generated["utf8Bytes"].as_u64().unwrap() as usize),
                );
                serde_json::to_vec(&base).unwrap()
            }
        };
        let result = decode_control_request(&data);
        if case["disposition"] == "accept" {
            let request = result.unwrap();
            assert_eq!(request.method_id(), Some(ControlMethod::PaneFocus));
        } else {
            assert_eq!(
                result.unwrap_err().response_code(),
                case["errorCode"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn control_method_catalog_matches_the_bounded_dispatch_surface() {
    let expected = [
        "ping",
        "tree",
        "identify",
        "capabilities",
        "event.list",
        "workspace.create",
        "workspace.select",
        "workspace.rename",
        "workspace.move",
        "workspace.close",
        "group.create",
        "group.select",
        "group.rename",
        "group.move",
        "group.close",
        "tab.create",
        "tab.select",
        "tab.rename",
        "tab.move",
        "tab.close",
        "pane.split",
        "pane.focus",
        "pane.rename",
        "pane.move",
        "pane.close",
        "pane.send",
        "pane.send_key",
        "pane.read_screen",
        "pane.notify",
        "agent.start",
        "agent.list",
        "agent.get",
        "agent.update",
        "agent.focus",
        "agent.resume",
        "todo.create",
        "todo.list",
        "todo.check",
        "todo.reopen",
        "todo.delete",
        "todo.export",
        "ssh.profile.list",
        "ssh.connect",
        "ssh.reconnect",
    ];
    let actual: HashSet<&str> = ControlMethod::ALL
        .iter()
        .map(|method| method.as_str())
        .collect();
    assert_eq!(actual, expected.into_iter().collect());
    assert_eq!(actual.len(), 44);
}

#[test]
fn cli_parser_maps_bounded_commands_without_shell_strings() {
    let mut environment = HashMap::new();
    environment.insert("HOME".to_owned(), "/tmp/kitmux-home".to_owned());
    environment.insert(
        "KITMUX_SOCKET_PATH".to_owned(),
        "/tmp/kitmux.sock".to_owned(),
    );

    let invocation = parse_cli(
        [
            "--json".to_owned(),
            "pane".to_owned(),
            "send".to_owned(),
            "current".to_owned(),
            "echo hello".to_owned(),
        ],
        &environment,
    )
    .unwrap();
    assert!(invocation.json);
    assert_eq!(invocation.request.method, "pane.send");
    assert_eq!(invocation.request.params["text"], "echo hello");

    let request = parse_cli(
        ["request".to_owned(), "workspace.create".to_owned()],
        &environment,
    )
    .unwrap();
    assert_eq!(
        request.request.method_id(),
        Some(ControlMethod::WorkspaceCreate)
    );
    assert!(matches!(
        parse_cli(
            ["request".to_owned(), "shell -c rm".to_owned()],
            &environment
        ),
        Err(CliParseError::Usage(_))
    ));
}

#[test]
fn control_response_codec_rejects_inconsistent_or_oversized_responses() {
    let success = ControlResponse::success("request-1", json!({"pong": true}));
    let encoded = encode_control_response(&success).unwrap();
    assert_eq!(decode_control_response(&encoded).unwrap(), success);

    let inconsistent =
        br#"{"version":1,"id":"x","ok":false,"result":{},"error":{"code":"bad","message":"bad"}}"#;
    assert_eq!(
        decode_control_response(inconsistent),
        Err(ControlCodecError::InvalidResponse)
    );
    assert_eq!(
        decode_control_response(&vec![b' '; CONTROL_MAX_RESPONSE_BYTES + 1]),
        Err(ControlCodecError::ResponseTooLarge)
    );
}

#[test]
fn newline_framer_handles_partial_multiple_crlf_and_incomplete_frames() {
    let mut decoder = LineFrameDecoder::request();
    assert!(decoder.push(b"{\"one\":").unwrap().is_empty());
    assert_eq!(
        decoder.push(b"1}\n{\"two\":2}\r\n").unwrap(),
        vec![b"{\"one\":1}".to_vec(), b"{\"two\":2}".to_vec()]
    );
    decoder.finish().unwrap();

    let mut incomplete = LineFrameDecoder::request();
    incomplete.push(b"unfinished").unwrap();
    assert_eq!(incomplete.finish(), Err(ControlCodecError::IncompleteFrame));

    let mut oversized = LineFrameDecoder::request();
    assert_eq!(
        oversized.push(&vec![b'x'; CONTROL_MAX_REQUEST_BYTES + 1]),
        Err(ControlCodecError::RequestTooLarge)
    );

    let mut exact_crlf = LineFrameDecoder::request();
    let mut maximum_frame = vec![b'x'; CONTROL_MAX_REQUEST_BYTES];
    maximum_frame.extend_from_slice(b"\r\n");
    assert_eq!(
        exact_crlf.push(&maximum_frame).unwrap(),
        vec![vec![b'x'; CONTROL_MAX_REQUEST_BYTES]]
    );
}

#[test]
fn frozen_command_catalog_is_exact_bounded_and_semantically_mapped() {
    let corpus = fixture("command-identifiers.json");
    let accepted = corpus["cases"][0]["identifiers"].as_array().unwrap();
    let actual: Vec<&str> = CommandId::ALL.iter().map(|id| id.as_str()).collect();
    let expected: Vec<&str> = accepted.iter().map(|id| id.as_str().unwrap()).collect();
    assert_eq!(actual, expected);
    assert_eq!(actual.iter().copied().collect::<HashSet<_>>().len(), 38);
    for identifier in &actual {
        assert_eq!(
            CommandId::from_str(identifier).unwrap().as_str(),
            *identifier
        );
    }
    for rejected in corpus["cases"][1]["identifiers"].as_array().unwrap() {
        assert!(CommandId::from_str(rejected.as_str().unwrap()).is_err());
    }
    assert_eq!(
        CommandId::from_str("pane.focus-left").unwrap().action(),
        SemanticAction::MovePaneFocus(kitmux_model::Direction::Left)
    );
}

#[test]
fn frozen_ssh_corpus_validates_documents_and_builds_data_only_review() {
    let corpus = fixture("ssh-profile-review.json");
    for case in corpus["cases"].as_array().unwrap() {
        if let Some(input) = case.get("input") {
            let result = decode_ssh_profiles(&serde_json::to_vec(input).unwrap());
            if case["disposition"] == "accept" {
                let document = result.unwrap();
                assert!(
                    document
                        .profiles
                        .iter()
                        .all(|profile| { profile.remote_command.is_none() })
                );
                let encoded = encode_ssh_profiles(document.clone()).unwrap();
                assert_eq!(decode_ssh_profiles(&encoded).unwrap(), document);
            } else {
                assert!(matches!(result, Err(SshCodecError::Invalid(_))));
            }
            continue;
        }

        let profile: SshProfile = serde_json::from_value(case["profile"].clone()).unwrap();
        let resolution = SshResolution::parse(
            &profile.host_alias,
            case["resolutionOutput"].as_str().unwrap(),
        )
        .unwrap();
        let review = resolution.review(&profile);
        let expected = &case["expectedReview"];
        assert_eq!(
            review.destination,
            expected["destination"].as_str().unwrap()
        );
        assert_eq!(review.host_alias, expected["hostAlias"].as_str().unwrap());
        assert_eq!(
            review.strict_host_key_checking,
            expected["strictHostKeyChecking"].as_str().unwrap()
        );
        assert_eq!(review.proxy_jump.as_deref(), expected["proxyJump"].as_str());
        assert_eq!(
            review.forwards.len(),
            expected["forwards"].as_u64().unwrap() as usize
        );
        assert_eq!(
            review.has_externally_listening_forward,
            expected["hasExternallyListeningForward"].as_bool().unwrap()
        );
        assert_eq!(
            review.requires_approval,
            expected["requiresApproval"].as_bool().unwrap()
        );
        assert_eq!(
            review.fingerprint,
            expected["fingerprint"].as_str().unwrap()
        );
    }
}

#[test]
fn ssh_codec_rejects_bounds_versions_duplicate_ids_and_preserves_unknown_fields() {
    assert_eq!(
        decode_ssh_profiles(br#"{"version":2,"profiles":[]}"#),
        Err(SshCodecError::UnsupportedVersion(2))
    );
    assert_eq!(
        decode_ssh_profiles(&vec![b' '; 1024 * 1024 + 1]),
        Err(SshCodecError::TooLarge)
    );
    let profile = json!({
        "id": "11111111-2222-3333-4444-555555555555",
        "name": "Production",
        "hostAlias": "prod",
        "createdAt": "2026-07-23T12:00:00Z",
        "updatedAt": "2026-07-23T12:00:00Z"
    });
    let duplicate = json!({"version": 1, "profiles": [profile.clone(), profile]});
    assert_eq!(
        decode_ssh_profiles(&serde_json::to_vec(&duplicate).unwrap()),
        Err(SshCodecError::Invalid("duplicate profile ID"))
    );
    let invalid_timestamp = json!({
        "version": 1,
        "profiles": [{
            "id": "11111111-2222-3333-4444-555555555555",
            "name": "Production",
            "hostAlias": "prod",
            "createdAt": "2026-02-30T12:00:00Z",
            "updatedAt": "2026-02-30T12:00:00Z"
        }]
    });
    assert_eq!(
        decode_ssh_profiles(&serde_json::to_vec(&invalid_timestamp).unwrap()),
        Err(SshCodecError::Invalid("invalid timestamps"))
    );

    let unknown = json!({
        "version": 1,
        "futureDocumentField": true,
        "profiles": [{
            "id": "11111111-2222-3333-4444-555555555555",
            "name": "Production",
            "hostAlias": "prod",
            "reviewedFingerprint": "INVALID",
            "createdAt": "2026-07-23T12:00:00Z",
            "updatedAt": "2026-07-23T12:00:00Z",
            "futureProfileField": 7
        }]
    });
    let document = decode_ssh_profiles(&serde_json::to_vec(&unknown).unwrap()).unwrap();
    assert_eq!(document.extra["futureDocumentField"], true);
    assert_eq!(document.profiles[0].extra["futureProfileField"], 7);
    assert_eq!(document.profiles[0].reviewed_fingerprint, None);
    let encoded: Value = serde_json::from_slice(&encode_ssh_profiles(document).unwrap()).unwrap();
    assert_eq!(encoded["futureDocumentField"], true);
    assert_eq!(encoded["profiles"][0]["futureProfileField"], 7);
}

#[test]
fn macos_state_import_preview_is_read_only_translates_paths_and_keeps_commands_inert() {
    let temp = TestDirectory::new();
    let linux_home = temp.path().join("home");
    let translated_cwd = linux_home.join("project");
    fs::create_dir_all(&translated_cwd).unwrap();
    let marker = temp.path().join("command-must-not-run");
    let state_path = temp.path().join("macos-state.json");
    let pane = "11111111-1111-1111-1111-111111111111";
    let source = json!({
        "version": 0,
        "activeWorkspaceIndex": 9,
        "createdWorkspaceCount": 0,
        "workspaces": [{
            "id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "name": "workspace 1",
            "activeTabGroupIndex": 7,
            "createdGroupCount": 0,
            "tabGroups": [{
                "name": "main",
                "activeTerminalTabIndex": 5,
                "terminalTabs": [{
                    "focusedPaneID": {"rawValue": pane},
                    "customTitle": "  ",
                    "root": {"pane": {"_0": {"rawValue": pane}}},
                    "paneDetails": {
                        pane: {
                            "surfaces": [{
                                "id": "22222222-2222-2222-2222-222222222222",
                                "kind": "terminal",
                                "cwd": "/Users/ethan/project",
                                "resumeCommand": format!("touch {}", marker.display())
                            }, {
                                "id": "33333333-3333-3333-3333-333333333333",
                                "kind": "browser",
                                "url": "https://example.com"
                            }, {
                                "id": "44444444-4444-4444-4444-444444444444",
                                "kind": "terminal",
                                "resumeCommand": "bad\ncommand"
                            }],
                            "activeSurfaceIndex": 99
                        }
                    }
                }]
            }]
        }]
    });
    fs::write(&state_path, serde_json::to_vec_pretty(&source).unwrap()).unwrap();
    let before = sha256_file(&state_path, 8 * 1024 * 1024).unwrap();
    let preview = preview_macos_state_file(&state_path, &linux_home).unwrap();
    let after = sha256_file(&state_path, 8 * 1024 * 1024).unwrap();

    assert_eq!(before, after);
    assert!(!marker.exists());
    assert_eq!(preview.source_sha256, before);
    assert_eq!(preview.inert_commands.len(), 1);
    assert!(preview.inert_commands[0].requires_explicit_approval);
    assert!(preview.translated.iter().any(|item| {
        item.field.ends_with("/cwd")
            && item.to == Value::String(translated_cwd.to_string_lossy().into_owned())
    }));
    assert!(
        preview
            .translated
            .iter()
            .any(|item| item.field == "/version")
    );
    assert!(
        preview
            .accepted
            .iter()
            .any(|item| item.field.ends_with("/url"))
    );
    assert!(preview.rejected.iter().any(|item| {
        item.field.ends_with("/resumeCommand") && item.detail.contains("contains controls")
    }));
}

#[test]
fn import_preview_rejects_newer_state_without_rewriting_and_bounds_inputs() {
    let temp = TestDirectory::new();
    let source = temp.path().join("newer.json");
    fs::write(&source, br#"{"version":99}"#).unwrap();
    let before = fs::read(&source).unwrap();
    let preview = preview_macos_state_file(&source, temp.path()).unwrap();
    assert_eq!(fs::read(&source).unwrap(), before);
    assert!(preview.accepted.is_empty());
    assert_eq!(preview.rejected.len(), 1);
    assert!(preview.rejected[0].detail.contains("newer than supported"));

    assert_eq!(
        kitmux_model::preview_macos_state(b"{}", Path::new("relative")),
        Err(ImportPreviewError::InvalidTargetHome)
    );
    assert_eq!(
        kitmux_model::preview_macos_state(&vec![b' '; 8 * 1024 * 1024 + 1], temp.path()),
        Err(ImportPreviewError::TooLarge)
    );
}

#[test]
fn xdg_paths_use_absolute_overrides_defaults_and_runtime_fallbacks() {
    let home = Path::new("/home/tester");
    let defaults = XdgPaths::resolve(&HashMap::new(), home).unwrap();
    assert_eq!(
        defaults.settings_file(),
        home.join(".config/kitmux/settings.json")
    );
    assert_eq!(
        defaults.state_file(),
        home.join(".local/state/kitmux/state.json")
    );

    let overrides = HashMap::from([
        ("XDG_CONFIG_HOME".to_owned(), "/cfg".to_owned()),
        ("XDG_DATA_HOME".to_owned(), "/data".to_owned()),
        ("XDG_STATE_HOME".to_owned(), "/state".to_owned()),
        ("XDG_CACHE_HOME".to_owned(), "/cache".to_owned()),
        ("XDG_RUNTIME_DIR".to_owned(), "relative".to_owned()),
    ]);
    let paths = XdgPaths::resolve(&overrides, home).unwrap();
    assert_eq!(paths.config_home, PathBuf::from("/cfg"));
    assert_eq!(
        UnixSocketAddress::resolve(&overrides, &paths, 501)
            .unwrap()
            .path(),
        Path::new("/tmp/kitmux-501/kitmux.sock")
    );

    let relative = HashMap::from([("XDG_CONFIG_HOME".to_owned(), "relative".to_owned())]);
    assert!(matches!(
        XdgPaths::resolve(&relative, home),
        Err(RuntimePathError::NotAbsolute("XDG_CONFIG_HOME"))
    ));
}

#[test]
fn socket_path_parent_must_be_owned_private_and_not_a_symlink() {
    let temp = TestDirectory::new();
    let uid = fs::metadata(temp.path()).unwrap().uid();
    let socket = UnixSocketAddress::new(temp.path().join("kitmux/kitmux.sock")).unwrap();
    socket.prepare_parent(uid).unwrap();
    assert_eq!(
        fs::metadata(temp.path().join("kitmux"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    fs::set_permissions(
        temp.path().join("kitmux"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(matches!(
        socket.prepare_parent(uid),
        Err(RuntimePathError::UnsafePermissions(0o755))
    ));

    let real = temp.path().join("real");
    fs::create_dir(&real).unwrap();
    let link = temp.path().join("link");
    symlink(&real, &link).unwrap();
    let linked_socket = UnixSocketAddress::new(link.join("kitmux.sock")).unwrap();
    assert!(matches!(
        linked_socket.prepare_parent(uid),
        Err(RuntimePathError::UnsafeRuntimeDirectory)
    ));
}

#[test]
fn socket_override_must_be_absolute_and_fit_linux_sun_path() {
    assert!(matches!(
        UnixSocketAddress::new(PathBuf::from("kitmux.sock")),
        Err(RuntimePathError::NotAbsolute("socket"))
    ));
    let oversized = PathBuf::from(format!("/tmp/{}", "x".repeat(108)));
    assert!(matches!(
        UnixSocketAddress::new(oversized),
        Err(RuntimePathError::SocketPathTooLong(_))
    ));
}

#[test]
fn hashing_and_bounded_reads_reject_oversize_and_symlink_inputs() {
    assert_eq!(
        sha256_bytes(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let temp = TestDirectory::new();
    let file = temp.path().join("data");
    fs::write(&file, b"abc").unwrap();
    assert_eq!(sha256_file(&file, 3).unwrap(), sha256_bytes(b"abc"));
    assert_eq!(read_bounded(&file, 3).unwrap(), b"abc");
    assert_eq!(
        read_bounded(&file, 2).unwrap_err().kind(),
        std::io::ErrorKind::FileTooLarge
    );
    let link = temp.path().join("link");
    symlink(&file, &link).unwrap();
    assert_eq!(
        read_bounded(&link, 3).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert_eq!(
        sha256_file(&link, 3).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
}

#[test]
fn atomic_private_write_replaces_files_with_private_mode_and_rejects_symlinks() {
    let temp = TestDirectory::new();
    let path = temp.path().join("nested/settings.json");
    atomic_write_private(&path, b"one").unwrap();
    atomic_write_private(&path, b"two").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"two");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let target = temp.path().join("target");
    fs::write(&target, b"safe").unwrap();
    let link = temp.path().join("settings-link");
    symlink(&target, &link).unwrap();
    assert!(matches!(
        atomic_write_private(&link, b"unsafe"),
        Err(AtomicWriteError::UnsafeDestination)
    ));
    assert_eq!(fs::read(target).unwrap(), b"safe");

    let linked_parent = temp.path().join("linked-parent");
    symlink(temp.path().join("nested"), &linked_parent).unwrap();
    assert!(matches!(
        atomic_write_private(&linked_parent.join("state.json"), b"unsafe"),
        Err(AtomicWriteError::UnsafeParent)
    ));
}

#[test]
fn polling_watcher_detects_create_in_place_atomic_replace_and_remove() {
    let temp = TestDirectory::new();
    let path = temp.path().join("settings.json");
    let mut watcher = PollingFileWatcher::new(path.clone(), 1024).unwrap();
    assert_eq!(watcher.poll().unwrap(), None);

    fs::write(&path, b"one").unwrap();
    assert_eq!(watcher.poll().unwrap(), Some(FileChange::Created));
    fs::write(&path, b"two").unwrap();
    assert_eq!(watcher.poll().unwrap(), Some(FileChange::Modified));
    atomic_write_private(&path, b"new").unwrap();
    assert_eq!(watcher.poll().unwrap(), Some(FileChange::Modified));
    fs::remove_file(&path).unwrap();
    assert_eq!(watcher.poll().unwrap(), Some(FileChange::Removed));
}
