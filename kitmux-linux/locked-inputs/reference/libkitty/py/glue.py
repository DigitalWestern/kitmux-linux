# libkitty Python glue: everything engine.c/session.c/render.c call, in one
# module running inside the embedded interpreter. Merges the proven spike code
# (spikes/00-proof-of-life, 01-headless-pty, 03-render) without the
# kitty_tests dependency, which must not ship.
import fcntl
import os
import re
import select
import signal
import struct
import subprocess
import termios
import threading
import time

_render = {}  # populated by render_init: fonts capsule, point size + cell dims
_sessions = set()


def _reap_child_in_background(pid):
    """Guarantee eventual waitpid without blocking AppKit's main thread."""
    def reap():
        while True:
            try:
                os.waitpid(pid, 0)
                return
            except InterruptedError:
                continue
            except ChildProcessError:
                return
    threading.Thread(target=reap, name=f'kitmux-reap-{pid}', daemon=True).start()

# v0.9 notifications: cap per-field size so an OSC-spamming child cannot
# balloon host memory or the sidebar.
_MAX_NOTIFICATION_FIELD = 2048

# v0.11 user variables: cap the decoded value so an OSC-spamming child cannot
# balloon host memory. The host re-enforces the real 2048-byte resume policy.
_MAX_USER_VAR_VALUE = 4096


def _clean_notification_field(text):
    # Strip C0 controls, DEL, and C1 controls; cap the length. Escape
    # sequences cannot survive into host UI strings.
    out = ''.join(
        ch for ch in text
        if ord(ch) >= 0x20 and not (0x7F <= ord(ch) <= 0x9F))
    return out[:_MAX_NOTIFICATION_FIELD]


def _parse_osc99(text):
    """Pragmatic single-part subset of kitty's OSC 99 protocol. Returns
    (title, body) or None to drop. Multi-part chunked commands (d=0),
    non-text payload types, ids, actions, queries, and close requests are
    dropped quietly."""
    metadata, sep, payload = text.partition(';')
    if not sep and '=' not in metadata:
        # Bare "OSC 99 ; message" (the shape Cmux documents).
        return metadata, ''
    meta = {}
    for entry in metadata.split(':'):
        if not entry:
            continue
        key, eq, value = entry.partition('=')
        if not eq:
            return None  # malformed metadata
        meta[key] = value
    if meta.get('d') == '0':
        return None  # chunked continuation: unsupported
    ptype = meta.get('p', 'title')
    if ptype not in ('title', 'body'):
        return None
    if meta.get('e') == '1':
        import base64
        try:
            payload = base64.standard_b64decode(payload).decode('utf-8', 'replace')
        except (ValueError, TypeError):
            return None
    return (payload, '') if ptype == 'title' else ('', payload)


def _apply_kitmux_defaults(opts, config_path):
    # F7: kitmux defaults for keys the user's config does not set. An
    # explicit user value always wins; only genuinely-unset keys change.
    explicitly_set = set()
    if config_path:
        try:
            from kitty.config import parse_config
            with open(config_path) as f:
                explicitly_set = set(parse_config(f))
        except OSError:
            pass
    from kitty.fast_data_types import Color
    if 'cursor' not in explicitly_set:
        opts.cursor = Color(255, 255, 255)   # fat white block by default
    if 'cursor_trail' not in explicitly_set:
        opts.cursor_trail = 1                # trail on by default
    if 'cursor_trail_decay' not in explicitly_set:
        opts.cursor_trail_decay = (0.1, 0.4)
    if 'inactive_text_alpha' not in explicitly_set:
        # Keep the blinking cursor as the primary split-focus cue. Kitty's
        # negative form fades only non-active splits, without dimming the
        # focused split merely because the macOS window is inactive.
        opts.inactive_text_alpha = -0.85


def _load_options(config_path):
    if config_path:
        from kitty.config import load_config
        opts = load_config(config_path)
    else:
        from kitty.config import finalize_keys, finalize_mouse_mappings
        from kitty.options.parse import merge_result_dicts
        from kitty.options.types import Options, defaults
        opts = Options(merge_result_dicts(defaults._asdict(), {
            'scrollback_pager_history_size': 1024, 'click_interval': 0.5}))
        finalize_keys(opts, {})
        finalize_mouse_mappings(opts, {})
    _apply_kitmux_defaults(opts, config_path)
    return opts


def init(config_path=None):
    from kitty.fast_data_types import set_options
    set_options(_load_options(config_path))
    return True


def reload_config(config_path=None):
    """S4: re-read kitty.conf and re-apply engine-consumed options to live
    sessions. Colors re-apply through each Screen's ColorProfile via kitty's
    own reload_from_opts; everything else engine-side (font family, scrollback
    sizing) keeps its current value — the host badges those honestly. Any
    failure raises BEFORE set_options, so the last good configuration stays
    fully active. kitty's loader silently skips a missing file, which would
    masquerade as "reset to defaults" — check existence explicitly instead.
    Returns the number of live sessions patched."""
    from kitty.fast_data_types import set_options
    if config_path and not os.path.isfile(config_path):
        raise FileNotFoundError(f'no kitty config at {config_path}')
    opts = _load_options(config_path)
    if config_path and config_path not in (opts.config_paths or ()):
        # kitty's loader silently skips an unreadable file, which here would
        # masquerade as "reset every option to defaults". config_paths records
        # what was actually read; refuse rather than guess.
        raise OSError(f'could not read kitty config at {config_path}')
    set_options(opts)
    from kitty import fast_data_types as fdt
    patched = 0
    for session in tuple(_sessions):
        session.screen.color_profile.reload_from_opts(opts)
        session.screen.mark_as_dirty()
        if session.stub is not None:
            # A render stub bakes background_opacity in at creation; refresh
            # it the way stock kitty's apply_options_update refreshes each OS
            # window. Plain memory write, no GL context needed.
            fdt.libkitty_refresh_window_stub_options(session.stub)
        patched += 1
    return patched


class Callbacks:
    # Screen's callback object. kitty's CALLBACK macro prints missing methods
    # to stderr and carries on, so only what real shells exercise needs to be
    # real; the rest are explicit no-ops to keep stderr clean.
    def __init__(self):
        self.wtcbuf = b''          # bytes Screen wants written back to the child
        self.pending_title = None
        self.pending_bells = 0
        self.pending_notifications = []  # [(title, body)] drained by pump
        self.pending_user_vars = []  # v0.11: [(key, value)] OSC 1337 SetUserVar
        self.color_profile = None
        self.screen = None         # set right after Screen creation

    def write(self, data):
        self.wtcbuf += bytes(data)

    def title_changed(self, data, is_base64=False):
        from kitty.window import process_title_from_child
        self.pending_title = process_title_from_child(data, is_base64, '')

    def on_bell(self):
        self.pending_bells += 1

    def on_da1(self):
        from kitty.fast_data_types import ESC_CSI, get_options
        from kitty.window import da1
        self.screen.send_escape_code_to_child(ESC_CSI, da1(get_options()))

    def request_capabilities(self, q):
        from kitty.terminfo import get_capabilities
        for c in get_capabilities(q, None):
            self.wtcbuf += c.encode('ascii')

    def desktop_notify(self, osc_code, raw_data):
        # kitty's vt-parser dispatches OSC 9/99/777/1337 here with everything
        # after "OSC <code>;". Interpretation mirrors kitty's host layer
        # (window.py/notifications.py): 777 requires and strips a "notify;"
        # prefix; 9 is title-only; 99 gets the single-part subset above.
        # OSC 1337 is not a notification: its SetUserVar subcommand (the only
        # one kitty's Screen surfaces here) is captured for the host; every
        # other OSC 1337 payload is ignored, as in kitty.
        text = bytes(raw_data).decode('utf-8', 'replace')
        if osc_code == 1337:
            self._capture_user_var(text)
            return
        if osc_code == 9:
            title, body = text, ''
        elif osc_code == 777:
            if not text.startswith('notify;'):
                return
            title, _, body = text[len('notify;'):].partition(';')
        elif osc_code == 99:
            parsed = _parse_osc99(text)
            if parsed is None:
                return
            title, body = parsed
        else:
            return
        title = _clean_notification_field(title)
        body = _clean_notification_field(body)
        if not title and not body:
            return
        self.pending_notifications.append((title, body))

    def _capture_user_var(self, text):
        # OSC 1337 ; SetUserVar=<name>[=<base64>]. No "=<base64>" clears the
        # var (value ""). The value is NOT control-cleaned here: it is not a
        # display string, and the host's validResumeCommand rejects (never
        # mutates) any control character, so cleaning would corrupt rather
        # than reject. Sizes are capped only to guard host memory; the real
        # policy lives host-side.
        if not text.startswith('SetUserVar='):
            return
        name, eq, b64 = text[len('SetUserVar='):].partition('=')
        if not name or len(name) > 256:
            return
        if not eq:
            value = ''          # bare name: clear the variable
        else:
            import base64
            try:
                value = base64.standard_b64decode(b64).decode('utf-8', 'replace')
            except (ValueError, TypeError):
                return          # malformed payload: drop, never guess
            value = value[:_MAX_USER_VAR_VALUE]
        self.pending_user_vars.append((name, value))

    def _noop(self, *a, **k):
        pass

    on_reset = notify_child_of_resize = osc_context = _noop
    set_dynamic_color = set_color_table_color = color_profile_popped = _noop
    icon_changed = clipboard_control = _noop
    cmd_output_marking = manipulate_title_stack = color_control = _noop
    file_transmission = report_color_scheme_preference = _noop
    on_activity_since_last_focus = _noop


def _vendored_terminfo():
    # The kitty checkout ships terminfo/x/xterm-kitty; exporting it makes
    # vim/less/tmux resolve TERM=xterm-kitty even where the OS database lacks it.
    import kitty
    tdir = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(kitty.__file__))), 'terminfo')
    return tdir if os.path.isdir(os.path.join(tdir, 'x')) else None


def _child_environment(argv, overrides=None):
    env = os.environ.copy()
    from kitty.fast_data_types import get_options
    from kitty.options.utils import DELETE_ENV_VAR
    opts = get_options()
    for key, value in opts.env.items():
        if value == DELETE_ENV_VAR:
            env.pop(key, None)
        else:
            env[key] = value
    # Kitmux owns terminal identity even if kitty.conf contains conflicting
    # env directives. PWD is similarly applied after chdir in Session.
    env['TERM'] = 'xterm-kitty'
    tinfo = _vendored_terminfo()
    if tinfo:
        env['TERMINFO'] = tinfo

    # Embedding-host identity is intentionally applied after kitty.conf env
    # directives. These values describe the exact child session being created
    # and must not drift because a shared config contains the same key.
    if overrides:
        env.update(overrides)

    # D2 needs only cwd reporting. Reuse kitty's existing zsh/bash/fish
    # integration setup, but disable its unrelated prompt/title/completion
    # features. Respect explicit integration opt-outs.
    disabled = {'disabled', 'no-rc', 'no-cwd'} & set(opts.shell_integration)
    if not disabled:
        from kitty.shell_integration import modify_shell_environ
        modify_shell_environ(opts, env, argv)
        if env.get('KITTY_SHELL_INTEGRATION'):
            env['KITTY_SHELL_INTEGRATION'] = (
                'no-cursor no-title no-prompt-mark no-complete no-sudo')
            # The patched zsh integration applies this only when the final
            # prompt left by /etc/zshrc + the user's rc is Apple's untouched
            # stock value. A custom .zshrc/theme wins automatically; setting
            # this variable to an empty value is also an explicit opt-out.
            if os.path.basename(argv[0]).lstrip('-') == 'zsh':
                env.setdefault('KITMUX_DEFAULT_PROMPT', '%1~ %# ')
    return env


def _configured_shell_argv():
    from kitty.fast_data_types import get_options
    from kitty.utils import resolved_shell
    opts = get_options()
    # Preserve Kitmux's established login+interactive default. An explicit
    # kitty shell is otherwise used verbatim; users opt into flags themselves.
    if getattr(opts, 'shell', '.') == '.':
        return [os.environ.get('SHELL') or resolved_shell(opts)[0], '-il']
    argv = resolved_shell(opts)
    if not argv:
        raise ValueError('kitty shell setting resolved to an empty command')
    return argv


def encode_key(session, key, shifted_key, alternate_key, mods, action, text):
    """Encode one key event exactly the way kitty's Window.encoded_key does:
    kitty's encoder plus this screen's live DECCKM + keyboard-protocol state."""
    from kitty.fast_data_types import SCROLL_FULL, encode_key_for_tty
    s = session.screen
    data = encode_key_for_tty(
        key=key, shifted_key=shifted_key, alternate_key=alternate_key,
        mods=mods, action=action, text=text or '',
        key_encoding_flags=s.current_key_encoding_flags(),
        cursor_key_mode=s.cursor_key_mode).encode('utf-8')
    # kitty parity (keys.c send_key_to_child): a key press while scrolled
    # up snaps the viewport back to the live bottom. Keys that encode to
    # nothing stand in for kitty's is_no_action_key check.
    if action == 1 and data and s.scrolled_by:
        s.scroll(SCROLL_FULL, False)
    return data


def option_as_alt():
    from kitty.fast_data_types import get_options
    return int(get_options().macos_option_as_alt)


def config_number(key):
    """Return a parsed numeric option, or None for unsupported value types.

    kitty colors are small value objects rather than numbers; expose their
    packed 0xRRGGBB representation so an embedding host can theme its own
    chrome without duplicating kitty's config parser.
    """
    from kitty.fast_data_types import get_options
    index = None
    if ':' in key:
        # F7: "key:index" reaches into tuple options (cursor_trail_decay).
        key, _, idx = key.partition(':')
        try:
            index = int(idx)
        except ValueError:
            return None
    value = getattr(get_options(), key, None)
    if value is None:
        return None
    if index is not None:
        if not isinstance(value, (tuple, list)) or not (0 <= index < len(value)):
            return None
        value = value[index]
    rgb = getattr(value, 'rgb', None)
    if rgb is not None:
        return float(rgb)
    if isinstance(value, (bool, int, float)):
        return float(value)
    return None


def config_string(key):
    """Return a parsed string option, or None for unsupported value types."""
    from kitty.fast_data_types import get_options
    value = getattr(get_options(), key, None)
    return value if isinstance(value, str) else None


def _feed(screen, data):
    data = memoryview(data)
    while data:
        dest = screen.test_create_write_buffer()
        n = screen.test_commit_write_buffer(data, dest)
        data = data[n:]
        screen.test_parse_written_data()


class Session:
    def __init__(self, lines, cols, argv=None, cwd=None, env=None):
        from kitty.fast_data_types import Screen
        cw, ch = _render.get('cw', 10), _render.get('ch', 20)
        self.callbacks = Callbacks()
        self.screen = Screen(self.callbacks, lines, cols, 1024, cw, ch, 0, self.callbacks)
        self.callbacks.screen = self.screen
        self.callbacks.color_profile = self.screen.color_profile
        self.argv = list(argv) if argv else _configured_shell_argv()
        import shutil
        executable = self.argv[0] if os.path.isabs(self.argv[0]) else shutil.which(self.argv[0])
        if not executable or not os.path.isfile(executable) or not os.access(executable, os.X_OK):
            raise ValueError(f'cannot launch terminal command: {self.argv[0]}')
        self.argv[0] = executable
        child_env = _child_environment(self.argv, env)
        spawn_cwd = cwd if cwd and os.path.isdir(cwd) else None
        pid, master = os.forkpty()
        if pid == 0:  # child
            try:
                # Keep the requested path (not os.getcwd(), which resolves
                # symlinks like /tmp -> /private/tmp) so the shell's logical
                # $PWD round-trips through save and restore.
                child_env['PWD'] = os.getcwd()
                if spawn_cwd:
                    try:
                        os.chdir(spawn_cwd)
                        child_env['PWD'] = spawn_cwd
                    except OSError:
                        pass
                os.execvpe(self.argv[0], self.argv, child_env)
            finally:
                os._exit(127)
        self.pid, self.master = pid, master
        self.child_alive = True
        self.exit_status = None
        # forkpty makes the child the process-group leader for this controlling
        # terminal. Keep the observed value rather than assuming pid forever:
        # this is the stable baseline for distinguishing shell vs foreground
        # job-control groups.
        # forkpty creates the child as the leader of the new controlling
        # terminal's session/process group, so its pid is the stable shell
        # group identity. Calling getpgid here races that child-side setup.
        self.shell_pgid = pid
        self.last_foreground_pgid = None
        self.stub = None       # render window stub (set by set_geometry)
        self.vao = None        # per-session cell VAO (kitty: one per window)
        self.geometry = None   # (left, top, right, bottom) within the framebuffer
        self.viewport = None   # (fb_width_px, fb_height_px)
        # v0.10 search: canonical runtime-only query/marker state belongs to
        # this session's Screen. The Swift host mirrors only field contents
        # and the last count for its search-bar UI.
        self.search_query = ''
        self.search_pattern = None
        self.search_match_count = 0
        self.search_case_sensitive = False
        self.search_uses_regex = False
        self.search_current_line = None
        self.search_current_occurrence = None
        self._set_pty_size(lines, cols)
        _sessions.add(self)

    def _set_pty_size(self, lines, cols):
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack('HHHH', lines, cols, 0, 0))

    def pump(self, timeout=0.0):
        """One pump pass. Returns (changed, title_or_None, bell_count,
        exit_status_or_None, notifications, user_vars, bytes_read) — the C layer turns
        these into callbacks. notifications is a list of (title, body) pairs
        (v0.9); user_vars is a list of (key, value) pairs (v0.11)."""
        changed = False
        bytes_read = 0
        if self.child_alive:
            self._observe_foreground_process_group()
            # Bound one host turn. A child that writes faster than we parse
            # must not monopolize AppKit's main queue and starve hidden panes.
            # DispatchSource remains readable and schedules another fair turn.
            reads_remaining = 1
            while reads_remaining:
                r, _, _ = select.select([self.master], [], [], timeout)
                if not r:
                    break
                timeout = 0.0
                try:
                    data = os.read(self.master, 65536)
                except OSError:      # EIO: child side closed
                    data = b''
                if not data:
                    self._reap()
                    break
                bytes_read += len(data)
                _feed(self.screen, data)
                changed = True
                self._observe_foreground_process_group()
                reads_remaining -= 1
            self._drain_screen_writes()
        cb = self.callbacks
        title, cb.pending_title = cb.pending_title, None
        bells, cb.pending_bells = cb.pending_bells, 0
        notifications, cb.pending_notifications = cb.pending_notifications, []
        user_vars, cb.pending_user_vars = cb.pending_user_vars, []
        exit_status = None if self.child_alive else self.exit_status
        return changed, title, bells, exit_status, notifications, user_vars, bytes_read

    def _foreground_process_group(self):
        if not self.child_alive:
            return None
        try:
            pgid = os.tcgetpgrp(self.master)
            return pgid if pgid > 0 else None
        except OSError:
            return None

    def _observe_foreground_process_group(self):
        pgid = self._foreground_process_group()
        if pgid is not None and pgid != self.shell_pgid:
            self.last_foreground_pgid = pgid
        return pgid

    @staticmethod
    def _process_group_is_stopped(pgid):
        """Recognize a previously-foreground stopped job without treating a
        running background job as foreground work. Failure is deliberately
        conservative (false): close confirmation must never become a hang or
        crash path."""
        try:
            result = subprocess.run(
                ['/bin/ps', '-axo', 'pgid=,stat='],
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                text=True, timeout=0.2, check=False)
        except (OSError, subprocess.SubprocessError):
            return False
        if result.returncode != 0:
            return False
        for line in result.stdout.splitlines():
            fields = line.split()
            if len(fields) < 2:
                continue
            try:
                same_group = int(fields[0]) == pgid
            except ValueError:
                continue
            if same_group and 'T' in fields[1]:
                return True
        return False

    def has_foreground_process(self):
        if not self.child_alive:
            return False
        pgid = self._observe_foreground_process_group()
        if pgid is not None and pgid != self.shell_pgid:
            try:
                os.killpg(pgid, 0)
                return True
            except PermissionError:
                return True
            except ProcessLookupError:
                return False
        previous = self.last_foreground_pgid
        return bool(previous and previous != self.shell_pgid and
                    self._process_group_is_stopped(previous))

    def _drain_screen_writes(self):
        # Terminal responses (DA1, cursor reports...) land in wtcbuf.
        if self.callbacks.wtcbuf and self.child_alive:
            os.write(self.master, self.callbacks.wtcbuf)
            self.callbacks.wtcbuf = b''

    def write_to_child(self, data):
        if self.child_alive:
            os.write(self.master, data)

    def paste(self, data):
        # Mirrors kitty Window.paste_text: sanitize per bracketed-paste
        # mode, then Screen.paste() wraps with 200~/201~ iff mode 2004 is
        # set. Drain immediately so the paste doesn't wait for a pump.
        if not data or not self.child_alive:
            return
        if self.screen.in_bracketed_paste_mode:
            from kitty.utils import sanitize_for_bracketed_paste
            data = sanitize_for_bracketed_paste(bytes(data))
        else:
            # Newline normalization for editors that choke on \n in pastes
            # (kitty issue #994).
            data = bytes(data).replace(b'\r\n', b'\n').replace(b'\n', b'\r')
        self.screen.paste(data)
        self._drain_screen_writes()

    def scroll(self, lines):
        # Mirrors kitty's wheel handling (mouse.c scroll_event): viewport
        # history scroll on the main screen — Screen.scroll clamps to the
        # history size and keeps all state in the Screen — and on the
        # alternate screen (vim, less) kitty's fake_scroll: encoded
        # Up/Down arrow key presses sent to the child.
        if not lines:
            return
        if self.screen.is_main_linebuf():
            self.screen.scroll(abs(lines), lines > 0)
        elif self.child_alive:
            from kitty.fast_data_types import encode_key_for_tty
            key = 0xE008 if lines > 0 else 0xE009  # kitty UP / DOWN
            flags = self.screen.current_key_encoding_flags()
            ckm = self.screen.cursor_key_mode
            chunks = []
            for _ in range(abs(lines)):
                for action in (1, 0):  # press, release (keys.c fake_scroll)
                    chunks.append(encode_key_for_tty(
                        key=key, action=action, key_encoding_flags=flags,
                        cursor_key_mode=ckm))
            data = ''.join(chunks).encode('utf-8')
            if data:
                os.write(self.master, data)

    def clear_scrollback(self):
        self.screen.clear_scrollback()

    def scrolled_by(self):
        return self.screen.scrolled_by

    def history_line_count(self):
        return self.screen.historybuf.count if self.screen.is_main_linebuf() else 0

    def cursor_cell(self):
        # F7: live cursor cell for the host's trail overlay. visible is false
        # while the viewport is scrolled back or DECTCEM hid the cursor.
        s = self.screen
        visible = bool(getattr(s, 'cursor_visible', True)) and not s.scrolled_by
        return int(s.cursor.x), int(s.cursor.y), visible

    def selection_start(self, column, row, in_left_half_of_cell, extend_mode=0):
        # extend_mode: 0 cell (drag), 1 word (double-click), 2 line
        # (triple-click). kitty's engine owns the word/line boundary logic.
        from kitty.fast_data_types import EXTEND_CELL, EXTEND_LINE, EXTEND_WORD
        mode = {0: EXTEND_CELL, 1: EXTEND_WORD, 2: EXTEND_LINE}.get(
            extend_mode, EXTEND_CELL)
        self.screen.start_selection(
            column, row, False, mode, in_left_half_of_cell)
        if mode != EXTEND_CELL:
            # start_selection only records the anchor; the word/line boundary
            # is realized in the update path. Fire one update at the same point
            # (still in progress, so a drag can extend by whole words/lines).
            self.screen.update_selection(
                column, row, in_left_half_of_cell, False, False)

    def selection_update(self, column, row, in_left_half_of_cell, ended):
        self.screen.update_selection(
            column, row, in_left_half_of_cell, ended, False)

    def selection_clear(self):
        self.screen.clear_selection()

    def selection_text(self):
        return ''.join(self.screen.text_for_selection(False, True))

    def _refresh_search_count(self):
        pattern = self.search_pattern
        if pattern is None:
            self.search_match_count = 0
            return 0
        screen = self.screen
        count = 0
        if screen.is_main_linebuf():
            for y in range(screen.historybuf.count):
                count += sum(1 for _ in pattern.finditer(str(screen.historybuf.line(y))))
        for y in range(screen.lines):
            count += sum(1 for _ in pattern.finditer(str(screen.line(y))))
        self.search_match_count = count
        return count

    @staticmethod
    def _regex_safety_error(text):
        if len(text.encode('utf-8')) > 512:
            return 'Regex is limited to 512 UTF-8 bytes'
        try:
            try:
                from re import _parser as sre_parse
            except ImportError:
                import sre_parse
            repeat_ops = {
                sre_parse.MAX_REPEAT,
                sre_parse.MIN_REPEAT,
                getattr(sre_parse, 'POSSESSIVE_REPEAT', object()),
            }
            reference_ops = {
                sre_parse.GROUPREF,
                getattr(sre_parse, 'GROUPREF_EXISTS', object()),
            }

            def walk(items, inside_repeat=False):
                for op, value in items:
                    if op in reference_ops:
                        return True
                    if op in repeat_ops:
                        if inside_repeat:
                            return True
                        if walk(value[2], True):
                            return True
                    elif op is sre_parse.SUBPATTERN:
                        if walk(value[-1], inside_repeat):
                            return True
                    elif op is sre_parse.BRANCH:
                        if any(walk(branch, inside_repeat) for branch in value[1]):
                            return True
                    elif op in (sre_parse.ASSERT, sre_parse.ASSERT_NOT):
                        if walk(value[1], inside_repeat):
                            return True
                return False

            if walk(sre_parse.parse(text)):
                return 'Regex uses nested repetition or backreferences'
        except (ValueError, OverflowError, RuntimeError, re.error) as e:
            return str(e) or 'Invalid regex'
        return None

    def _search_marker(self, current_line=None, current_occurrence=None):
        from kitty.fast_data_types import set_uint_at_address
        pattern = self.search_pattern
        seen_current_text = 0

        def marker(text, left_address, right_address, color_address):
            nonlocal seen_current_text
            is_current = current_line is not None and text == current_line \
                and seen_current_text == current_occurrence
            if current_line is not None and text == current_line:
                seen_current_text += 1
            color = 2 if is_current else 1
            set_uint_at_address(color_address, color)
            for match in pattern.finditer(text):
                set_uint_at_address(left_address, match.start())
                set_uint_at_address(right_address, match.end() - 1)
                yield

        return marker

    def _mark_current_visible_line(self, backwards=False):
        candidates = []
        for y in range(self.screen.lines):
            if y < self.screen.scrolled_by:
                line = str(self.screen.historybuf.line(
                    self.screen.scrolled_by - 1 - y))
            else:
                line = str(self.screen.line(y - self.screen.scrolled_by))
            if self.search_pattern.search(line) and line != self.search_current_line:
                candidates.append((y, line))
        if not candidates:
            return False
        row, self.search_current_line = candidates[0] if backwards else candidates[-1]
        if row < self.screen.scrolled_by:
            history_index = self.screen.scrolled_by - 1 - row
            before = [str(self.screen.line(y)) for y in range(self.screen.lines)]
            before += [str(self.screen.historybuf.line(y))
                       for y in range(history_index)]
        else:
            main_index = row - self.screen.scrolled_by
            before = [str(self.screen.line(y)) for y in range(main_index)]
        self.search_current_occurrence = sum(
            1 for line in before if line == self.search_current_line)
        self.screen.set_marker(self._search_marker(
            self.search_current_line, self.search_current_occurrence))
        return True

    def search_set(self, query, case_sensitive=False, regex=False):
        try:
            text = bytes(query).decode('utf-8') if not isinstance(query, str) else query
        except UnicodeDecodeError:
            text = ''
        if not text:
            self.search_clear()
            return 0, ''
        if regex:
            error = self._regex_safety_error(text)
            if error:
                self.search_clear()
                return 0, error
        # kitty's native marker callback owns Unicode-to-cell mapping (wide
        # cells, tabs, combining characters). Mark 1 is every result; mark 2
        # is the current line-level navigation result.
        from kitty.fast_data_types import SCROLL_FULL
        flags = re.UNICODE | (0 if case_sensitive else re.IGNORECASE)
        try:
            pattern = re.compile(text if regex else re.escape(text), flags)
        except re.error as e:
            self.search_clear()
            return 0, str(e) or 'Invalid regex'
        self.search_query = text
        self.search_pattern = pattern
        self.search_case_sensitive = bool(case_sensitive)
        self.search_uses_regex = bool(regex)
        self.search_current_line = None
        self.search_current_occurrence = None
        self.screen.set_marker(self._search_marker())
        count = self._refresh_search_count()
        if count and self.screen.is_main_linebuf():
            # Query edits always start from live output. Stay there when it
            # contains a match; otherwise reveal the newest history match.
            self.screen.scroll(SCROLL_FULL, False)
            if not any(mark == 1 for _, _, mark in self.screen.marked_cells()):
                self.screen.scroll_to_next_mark(1, True)
            self._mark_current_visible_line(False)
        return count, ''

    def search_next(self, backwards=False):
        if self.search_pattern is None or self._refresh_search_count() == 0:
            return False
        screen = self.screen
        if not screen.is_main_linebuf():
            return False
        # Navigation uses mark 1 for every result, matching kitty's proven
        # line-level primitive. Once the viewport moves, recolor the chosen
        # visible result to mark 2.
        self.search_current_line = None
        screen.set_marker(self._search_marker())
        if screen.scroll_to_next_mark(1, bool(backwards)):
            return self._mark_current_visible_line(bool(backwards))
        # Wrap at either end. kitty's primitive navigates matching lines, so
        # multiple occurrences on one terminal row remain highlighted but
        # count as one navigation stop.
        from kitty.fast_data_types import SCROLL_FULL
        screen.scroll(SCROLL_FULL, not backwards)
        if not screen.scroll_to_next_mark(1, bool(backwards)):
            return False
        return self._mark_current_visible_line(bool(backwards))

    def search_refresh(self):
        return self._refresh_search_count()

    def search_visible_mark_count(self, mark):
        return sum(1 for _, _, cell_mark in self.screen.marked_cells()
                   if cell_mark == int(mark))

    def search_clear(self):
        self.search_query = ''
        self.search_pattern = None
        self.search_match_count = 0
        self.search_case_sensitive = False
        self.search_uses_regex = False
        self.search_current_line = None
        self.search_current_occurrence = None
        self.screen.set_marker()

    def mouse_event(self, cell_x, cell_y, button, action, mods, pixel_x, pixel_y):
        # kitty's send_mouse_event is the whole reporting engine: it gates
        # on the Screen's tracking mode, encodes in the active protocol,
        # and writes to the child. Returns whether the application consumed
        # the event; False has no side effects, so the caller can fall back
        # to local selection/scrolling.
        if not self.child_alive:
            return False
        from kitty.fast_data_types import send_mouse_event, PRESS, RELEASE, DRAG, MOVE
        actions = {0: PRESS, 1: RELEASE, 2: DRAG, 3: MOVE}
        if action not in actions:
            return False
        # libkitty mods (kitty key bits: shift=1, alt=2, ctrl=4) to the GLFW
        # bits the encoder expects (shift=1, ctrl=2, alt=4).
        glfw_mods = ((1 if mods & 0x1 else 0)
                     | (2 if mods & 0x4 else 0)
                     | (4 if mods & 0x2 else 0))
        handled = send_mouse_event(
            self.screen, cell_x, cell_y, button, actions[action], glfw_mods,
            pixel_x, pixel_y)
        if handled:
            self._drain_screen_writes()
        return bool(handled)

    def reported_cwd(self):
        raw = self.screen.last_reported_cwd
        if not raw:
            return ''
        try:
            from kitty.utils import path_from_osc7_url
            return path_from_osc7_url(raw)
        except (UnicodeDecodeError, ValueError):
            return ''

    def resize(self, lines, cols):
        # kitty's own calling discipline: Screen.resize rebuilds (rewraps,
        # resets the scrollback viewport) even at identical dimensions, so
        # kitty only invokes it on a real change. set_geometry runs on
        # every relayout — without this guard, switching tabs/workspaces
        # would reset every pane's scroll position.
        if (lines, cols) == (self.screen.lines, self.screen.columns):
            return
        self.screen.resize(lines, cols)
        if self.child_alive:
            self._set_pty_size(lines, cols)

    def text(self):
        return '\n'.join(str(self.screen.line(i)) for i in range(self.screen.lines))

    def line_wraps(self):
        return bytes(
            int(self.screen.visual_line(i).last_char_has_wrapped_flag())
            for i in range(self.screen.lines))

    def scrollback_text(self, max_lines, max_bytes):
        """03B spike: bounded, plain main-screen + history snapshot.

        Read only the newest requested physical rows, so both returned data and
        temporary allocations are bounded before UTF-8 byte trimming. History
        index zero is kitty's newest row; rows are reversed back into display
        order. Alternate-screen state deliberately returns no snapshot:
        exporting an editor/pager screen as durable shell history is misleading.
        """
        max_lines, max_bytes = int(max_lines), int(max_bytes)
        if max_lines <= 0 or max_bytes <= 0 or not self.screen.is_main_linebuf():
            return ''
        screen = self.screen
        live = []
        for y in range(screen.lines):
            line = screen.line(y)  # live/non-visual line, independent of viewport
            live.append((str(line), bool(line.last_char_has_wrapped_flag())))
        while live and not live[-1][0]:
            live.pop()             # terminal padding is not meaningful history
        live = live[-max_lines:]
        remaining = max_lines - len(live)
        history = []
        for y in range(min(remaining, screen.historybuf.count) - 1, -1, -1):
            line = screen.historybuf.line(y)
            history.append((str(line), bool(line.last_char_has_wrapped_flag())))
        rows = history + live
        if not rows:
            return ''
        chunks = []
        for text, wraps in rows:
            chunks.append(text)
            if not wraps:
                chunks.append('\n')
        data = ''.join(chunks).rstrip('\n').encode('utf-8')
        if len(data) > max_bytes:
            # Keep the newest bytes and discard any partial leading codepoint.
            data = data[-max_bytes:]
            return data.decode('utf-8', errors='ignore')
        return data.decode('utf-8')

    def replay_plain_text(self, data):
        """03B spike: display inert text without writing to the child PTY.

        Only printable Unicode, tabs, and line breaks survive. In particular,
        terminal escape/control sequences cannot become active during replay.
        CRLF gives terminal-output semantics while keeping the next line at
        column zero. This mutates only the Screen; the shell never receives it.
        """
        if not data:
            return False
        text = bytes(data).decode('utf-8', errors='replace')
        text = ''.join(
            ch for ch in text
            if ch in ('\n', '\t') or (ord(ch) >= 0x20 and ord(ch) != 0x7f))
        if not text:
            return False
        normalized = text.replace('\r\n', '\n').replace('\r', '\n')
        _feed(self.screen, normalized.replace('\n', '\r\n').encode('utf-8'))
        return True

    def fileno(self):
        return self.master

    def child_pid_value(self):
        return self.pid

    def _reap(self):
        if not self.child_alive:
            return
        self.child_alive = False
        try:
            # Blocking is acceptable here: we only reach this after EOF/EIO on
            # the master, which in practice means the child is exiting.
            _, status = os.waitpid(self.pid, 0)
            self.exit_status = os.waitstatus_to_exitcode(status)
        except ChildProcessError:
            self.exit_status = -1

    def close(self):
        _sessions.discard(self)
        if self.vao is not None:
            # Host contract: rendered sessions are closed with their GL
            # context current (remove_vao touches GL objects).
            from kitty import fast_data_types as fdt
            fdt.libkitty_remove_vao(self.vao)
            self.vao = None
        try:
            os.close(self.master)
        except OSError:
            pass
        if self.child_alive:
            self.child_alive = False

            def reap_if_ready():
                try:
                    waited, status = os.waitpid(self.pid, os.WNOHANG)
                except ChildProcessError:
                    self.exit_status = -1
                    return True
                except InterruptedError:
                    return False
                if waited:
                    self.exit_status = os.waitstatus_to_exitcode(status)
                    return True
                return False

            def signal_process_group(sig):
                try:
                    pgid = os.getpgid(self.pid)
                    # Never signal the host's process group if a child changed
                    # groups unexpectedly during startup.
                    if pgid != os.getpgrp():
                        os.killpg(pgid, sig)
                    else:
                        os.kill(self.pid, sig)
                except ProcessLookupError:
                    return
                except PermissionError:
                    # A group can contain a process we cannot signal. The
                    # direct child is always ours and is sufficient to unblock
                    # waitpid; closing the PTY handles its remaining jobs.
                    try:
                        os.kill(self.pid, sig)
                    except ProcessLookupError:
                        pass

            # Never block the host indefinitely during pane/tab shutdown.
            # Give SIGHUP 0.1s, then SIGKILL 0.2s, polling waitpid throughout.
            # This is per pane, so longer waits compound into a GUI freeze when
            # a large split/workspace closes.
            if not reap_if_ready():
                signal_process_group(signal.SIGHUP)
                for _ in range(10):
                    if reap_if_ready():
                        break
                    time.sleep(0.01)
                else:
                    signal_process_group(signal.SIGKILL)
                    for _ in range(20):
                        if reap_if_ready():
                            break
                        time.sleep(0.01)
                    else:
                        # Rare kernel/process-group delays must not freeze bulk
                        # GUI teardown. A dedicated waiter still guarantees the
                        # direct child cannot remain a zombie.
                        _reap_child_in_background(self.pid)


# ---- rendering (requires the host's GL context to be current) ----

def render_init(scale):
    from kitty import fast_data_types as fdt
    from kitty.fonts.render import set_font_family
    from kitty.shaders import load_shader_programs
    opts = fdt.get_options()
    fdt.libkitty_gl_init()
    load_shader_programs()
    set_font_family(opts)
    cap, cw, ch = fdt.libkitty_create_fonts_data(opts.font_size, 96.0 * scale, 96.0 * scale)
    _render.update(cap=cap, size=opts.font_size, scale=scale, cw=cw, ch=ch)
    return opts.font_size, cw, ch


def render_set_font_size(size):
    from kitty import fast_data_types as fdt
    # load_fonts_data may trim/move the previous font group. Drop every stub
    # that points into that group before asking kitty for its replacement.
    sessions = tuple(_sessions)
    for session in sessions:
        session.stub = None
    dpi = 96.0 * _render['scale']
    cap, cw, ch = fdt.libkitty_create_fonts_data(size, dpi, dpi)
    _render.update(cap=cap, size=size, cw=cw, ch=ch)
    for session in sessions:
        fdt.libkitty_set_screen_cell_size(session.screen, cw, ch)
    return cw, ch


def set_viewport(session, fb_w, fb_h):
    # v0 behavior: the session fills the whole framebuffer.
    set_geometry(session, fb_w, fb_h, 0, 0, fb_w, fb_h)


def release_render_resources(session):
    # v0.21: drop this session's context-local GL objects so it can be drawn
    # by a different GL context (multi-window). Only the VAO qualifies:
    #
    #   - shader programs, the font atlas, and VAO buffers live in the context
    #     share group and stay valid everywhere;
    #   - the VAO does not. glGenVertexArrays names are per-context, while
    #     kitty's vaos[] registry is process-global, so deleting a slot while
    #     the WRONG context is current deletes an unrelated VAO in that context
    #     and leaks this one. The caller must make the creating context current
    #     first; libkitty cannot detect the violation.
    #   - session.stub is a plain OSWindow struct with no GL objects, so it is
    #     deliberately left alone and merely resized by the next set_geometry.
    #
    # The lazy create in set_geometry rebuilds the VAO in whatever context is
    # current then.
    from kitty import fast_data_types as fdt
    if session.vao is not None:
        fdt.libkitty_remove_vao(session.vao)
        session.vao = None
    # The replacement VAO gets freshly allocated, EMPTY buffers. Marking the
    # screen dirty alone is NOT enough: that re-uploads cell data (so the
    # background appears) while sprite positions stay cached, and the pane
    # renders with no glyphs at all in the destination context. Measured, not
    # assumed -- see docs/history for Task 0.
    #
    # Force the same GPU reload without the font-change path's image rescale:
    # screen_rescale_images removes cell-image placements, even when the cell
    # size did not change.
    fdt.libkitty_force_screen_gpu_reload(session.screen)
    session.screen.mark_as_dirty()


def set_geometry(session, fb_w, fb_h, left, top, right, bottom):
    # v0.2: place this session's cells in a sub-rect of a larger framebuffer
    # (kitty's own model: many Windows composited into one OSWindow).
    from kitty import fast_data_types as fdt
    cw, ch = _render['cw'], _render['ch']
    session.resize(max(1, (bottom - top) // ch), max(2, (right - left) // cw))
    if session.stub is None:
        session.stub = fdt.libkitty_create_window_stub(fb_w, fb_h, _render['cap'])
    else:
        fdt.libkitty_resize_window_stub(session.stub, fb_w, fb_h)
    if session.vao is None:
        session.vao = fdt.libkitty_create_cell_vao()
    session.viewport = (fb_w, fb_h)
    session.geometry = (left, top, right, bottom)


def draw(session, cursor_visible=True, pane_active=True,
         window_focused=True, single_pane=True):
    from kitty import fast_data_types as fdt
    s = session.screen
    cw, ch = _render['cw'], _render['ch']
    left, top, _, _ = session.geometry
    # Kitmux supplies the blink phase because the extracted renderer has no
    # child-monitor redraw clock. Temporarily hiding DECTCEM for one draw lets
    # the existing render shim honor that phase without mutating terminal
    # state or requiring another vendored-kitty patch.
    restore_cursor = bool(s.cursor_visible)
    host_may_hide = (
        not cursor_visible and restore_cursor
        and bool(getattr(s.cursor, 'blink', True)))
    if host_may_hide:
        s.cursor_visible = False
    try:
        fdt.libkitty_draw_screen(
            s, session.stub, session.vao,
            left, top, left + s.columns * cw, top + s.lines * ch,
            pane_active, window_focused, single_pane)
    finally:
        if host_may_hide:
            s.cursor_visible = True
