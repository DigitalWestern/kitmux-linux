use kitmux_model::{
    CommandId, NavigationTarget, PasteConfirmationReason, PixelRect, ShortcutAction, ShortcutChord,
    ShortcutMap, TerminalCellCoordinate, TerminalUrlSegment, accumulate_scroll_lines,
    command_palette_matches, decode_settings, detected_url, namespaced_number_target,
    paste_confirmation_reason, terminal_cell, terminal_cell_scaled,
};

#[test]
fn paste_safety_matches_the_portable_contract() {
    assert_eq!(paste_confirmation_reason("git status\n", 8192), None);
    assert_eq!(
        paste_confirmation_reason("cd foo\nmake\tbuild\r\n", 8192),
        None
    );
    assert_eq!(
        paste_confirmation_reason(&"a".repeat(8193), 8192),
        Some(PasteConfirmationReason::Large { bytes: 8193 })
    );
    assert_eq!(paste_confirmation_reason(&"a".repeat(8192), 8192), None);
    assert_eq!(
        paste_confirmation_reason("safe\u{1b}[2Jclear", 8192),
        Some(PasteConfirmationReason::ControlCharacters)
    );
    assert_eq!(
        paste_confirmation_reason("nul\0byte", 8192),
        Some(PasteConfirmationReason::ControlCharacters)
    );
    assert_eq!(
        paste_confirmation_reason("del\u{7f}byte", 8192),
        Some(PasteConfirmationReason::ControlCharacters)
    );
}

#[test]
fn coordinates_map_exactly_and_clamp_at_scale() {
    let frame = PixelRect::new(100, 50, 103, 63);
    assert_eq!(
        terminal_cell(105.0, 70.0, frame, 10, 20),
        Some(TerminalCellCoordinate {
            column: 0,
            row: 1,
            in_left_half: false,
            pixel_x: 5,
            pixel_y: 20,
        })
    );
    assert_eq!(
        terminal_cell(10_000.0, 10_000.0, frame, 10, 20)
            .unwrap()
            .column,
        9
    );
    assert_eq!(
        terminal_cell(140.0, 90.0, PixelRect::new(100, 50, 200, 120), 20, 40)
            .unwrap()
            .row,
        1
    );
    assert_eq!(
        terminal_cell_scaled(25.0, 12.5, 2.0, PixelRect::new(0, 0, 200, 100), 10, 10).unwrap(),
        TerminalCellCoordinate {
            column: 5,
            row: 2,
            in_left_half: true,
            pixel_x: 50,
            pixel_y: 25,
        }
    );
    for (scale, padding, cell_width, cell_height) in
        [(1.0, 8, 8, 14), (1.5, 12, 12, 21), (2.0, 16, 16, 28)]
    {
        let frame = PixelRect::new(padding, padding, cell_width * 23, cell_height * 6);
        assert_eq!(
            terminal_cell_scaled(8.0, 8.0, scale, frame, cell_width, cell_height)
                .unwrap()
                .column,
            0
        );
        assert_eq!(
            terminal_cell_scaled(10_000.0, 10_000.0, scale, frame, cell_width, cell_height)
                .unwrap()
                .column,
            22
        );
    }
    assert!(terminal_cell_scaled(1.0, 1.0, 0.0, frame, 10, 20).is_none());
    assert!(terminal_cell(0.0, 0.0, PixelRect::new(0, 0, 19, 20), 10, 20).is_none());
}

#[test]
fn smooth_scroll_keeps_fractional_remainder() {
    let mut residue = 0.0;
    assert_eq!(accumulate_scroll_lines(7.5, 10.0, &mut residue), 0);
    assert_eq!(accumulate_scroll_lines(7.5, 10.0, &mut residue), 1);
    assert_eq!(residue, 5.0);
    assert_eq!(accumulate_scroll_lines(-25.0, 10.0, &mut residue), -2);
    assert_eq!(residue, 0.0);
}

#[test]
fn shortcut_matching_rejects_extra_modifiers() {
    let map = ShortcutMap::linux_defaults();
    let copy = ShortcutChord {
        key: 'c',
        control: true,
        shift: true,
        alt: false,
        super_key: false,
    };
    assert_eq!(map.resolve(copy), Some(ShortcutAction::Copy));
    assert_eq!(map.resolve(ShortcutChord { alt: true, ..copy }), None);
    assert_eq!(
        map.resolve(ShortcutChord {
            shift: false,
            ..copy
        }),
        None
    );
    assert_eq!(
        map.resolve(ShortcutChord {
            key: 'c',
            control: true,
            shift: false,
            alt: false,
            super_key: false,
        }),
        None
    );
    let ambiguous =
        ShortcutMap::from_bindings([(copy, ShortcutAction::Copy), (copy, ShortcutAction::Paste)]);
    assert_eq!(ambiguous.resolve(copy), None);

    let expected = [
        ('c', true, ShortcutAction::Copy),
        ('v', true, ShortcutAction::Paste),
        ('f', true, ShortcutAction::Search),
        ('p', true, ShortcutAction::CommandPalette),
        ('+', false, ShortcutAction::FontLarger),
        ('-', false, ShortcutAction::FontSmaller),
        ('0', false, ShortcutAction::FontReset),
        ('l', true, ShortcutAction::ClearScrollback),
    ];
    for (key, shift, action) in expected {
        assert_eq!(
            map.resolve(ShortcutChord {
                key,
                control: true,
                shift,
                alt: false,
                super_key: false,
            }),
            Some(action)
        );
    }
}

#[test]
fn command_palette_filtering_preserves_catalog_order_and_ranks_prefixes() {
    assert_eq!(command_palette_matches(""), CommandId::ALL);
    assert_eq!(
        command_palette_matches("pane.focus"),
        [
            CommandId::PaneFocusNext,
            CommandId::PaneFocusPrevious,
            CommandId::PaneFocusLeft,
            CommandId::PaneFocusRight,
            CommandId::PaneFocusUp,
            CommandId::PaneFocusDown,
        ]
    );
    assert_eq!(
        command_palette_matches("new-tab"),
        [CommandId::TerminalNewTab]
    );
}

#[test]
fn linux_shortcut_settings_override_defaults_without_stealing_plain_control_keys() {
    let settings = decode_settings(
        br#"{
          "linuxShortcutBindings": {
            "terminal.find": {"key":"G","control":true,"shift":true},
            "terminal.copy": {"key":"v","control":true,"shift":true},
            "font.increase": {"key":"=","control":true,"shift":true},
            "font.decrease": {"key":"c","control":true},
            "font.reset": {"key":"x","shift":true},
            "workspace.new": {"key":"g","super":true}
          }
        }"#,
    )
    .unwrap();
    let map = ShortcutMap::linux_from_settings(&settings);
    let chord = |key, shift| ShortcutChord {
        key,
        control: true,
        shift,
        alt: false,
        super_key: false,
    };

    assert_eq!(map.resolve(chord('g', true)), Some(ShortcutAction::Search));
    assert_eq!(map.resolve(chord('f', true)), None);
    assert_eq!(map.resolve(chord('v', true)), None);
    assert_eq!(
        map.resolve(chord('+', false)),
        Some(ShortcutAction::FontLarger)
    );
    assert_eq!(
        map.resolve(chord('-', false)),
        Some(ShortcutAction::FontSmaller)
    );
    assert_eq!(
        map.resolve(chord('0', false)),
        Some(ShortcutAction::FontReset)
    );
    assert_eq!(map.resolve(chord('c', false)), None);
    assert_eq!(
        map.resolve(ShortcutChord {
            key: 'g',
            control: false,
            shift: false,
            alt: false,
            super_key: true,
        }),
        Some(ShortcutAction::Navigation(CommandId::WorkspaceNew))
    );
    assert_eq!(
        map.resolve(ShortcutChord {
            key: 'n',
            control: false,
            shift: false,
            alt: false,
            super_key: true,
        }),
        None
    );
}

#[test]
fn navigation_number_chords_use_separate_linux_safe_namespaces() {
    let chord = |key, alt, super_key| ShortcutChord {
        key,
        control: false,
        shift: false,
        alt,
        super_key,
    };
    assert_eq!(
        namespaced_number_target(chord('1', false, true)),
        Some(NavigationTarget::Workspace(0))
    );
    assert_eq!(
        namespaced_number_target(chord('9', true, false)),
        Some(NavigationTarget::TerminalTab(8))
    );
    assert_eq!(namespaced_number_target(chord('0', false, true)), None);
    assert_eq!(namespaced_number_target(chord('1', true, true)), None);
    assert_eq!(
        namespaced_number_target(ShortcutChord {
            control: true,
            ..chord('1', false, true)
        }),
        None
    );
}

#[test]
fn wrapped_url_resolves_from_every_segment() {
    let rows = vec![
        "https://exam".to_owned(),
        "ple.com/abc".to_owned(),
        String::new(),
    ];
    let head = detected_url(&rows, 0, 3, 12, None).unwrap();
    let tail = detected_url(&rows, 1, 4, 12, None).unwrap();
    assert_eq!(head, tail);
    assert_eq!(head.url, "https://example.com/abc");
    assert_eq!(
        head.segments,
        vec![
            TerminalUrlSegment {
                row: 0,
                columns: 0..12
            },
            TerminalUrlSegment {
                row: 1,
                columns: 0..11
            },
        ]
    );
    assert_eq!(
        detected_url(&["mail me@example.com, please".to_owned()], 0, 8, 40, None)
            .unwrap()
            .url,
        "mailto:me@example.com"
    );
    assert!(detected_url(&rows, 1, 4, 12, Some(&[false, false, false])).is_none());
    assert!(detected_url(&["javascript:alert(1)".to_owned()], 0, 3, 30, None).is_none());
}
