# Keyboard-encoding verification against real programs (zsh, vim, less, tmux).
# Run: PYTHONPATH=vendor/kitty:libkitty/py python3 -m pytest libkitty/tests/test_keys.py -v
import os
import shutil
import sys
import tempfile
import time

import pytest

_here = os.path.dirname(os.path.abspath(__file__))
_root = os.path.dirname(os.path.dirname(_here))
sys.path.insert(0, os.path.join(_root, 'vendor', 'kitty'))
sys.path.insert(0, os.path.join(_root, 'libkitty', 'py'))
import glue  # noqa: E402

# kitty functional keys / mods (values from vendored glfw3.h)
FKEY_ESCAPE, FKEY_ENTER = 0xE000, 0xE001
FKEY_LEFT, FKEY_RIGHT, FKEY_UP, FKEY_DOWN = 0xE006, 0xE007, 0xE008, 0xE009
FKEY_PAGE_DOWN, FKEY_HOME, FKEY_END = 0xE00B, 0xE00C, 0xE00D
FKEY_F5 = 0xE018
SHIFT, ALT, CTRL = 0x1, 0x2, 0x4
PRESS = 1


@pytest.fixture(scope='module', autouse=True)
def engine():
    glue.init()
    yield


def make_session(argv, lines=12, cols=70):
    return glue.Session(lines, cols, argv)


def enc(s, key, mods=0, text=''):
    return glue.encode_key(s, key, 0, 0, mods, PRESS, text)


def send_key(s, key, mods=0, text=''):
    s.write_to_child(enc(s, key, mods, text))


def pump_until(s, predicate, timeout=8.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        s.pump(0.05)
        if predicate():
            return True
    return False


def screen_has(s, needle):
    return lambda: needle in s.text()


def test_legacy_encodings():
    s = make_session(['/bin/sh', '-c', 'sleep 60'])
    try:
        assert enc(s, FKEY_UP) == b'\x1b[A'
        assert enc(s, FKEY_LEFT, SHIFT) == b'\x1b[1;2D'
        assert enc(s, FKEY_LEFT, CTRL) == b'\x1b[1;5D'
        assert enc(s, FKEY_HOME) == b'\x1b[H'
        assert enc(s, FKEY_END) == b'\x1b[F'
        assert enc(s, FKEY_F5) == b'\x1b[15~'
        # Alt-as-modifier events carry NO text (kitty's cocoa convention:
        # text suppressed for alt chords when option_as_alt applies) — the
        # encoder then synthesizes ESC-prefix. Passing composed text instead
        # means "Option produced a character; send it verbatim".
        assert enc(s, ord('b'), ALT) == b'\x1bb'           # word-back in shells
        assert enc(s, ord('c'), CTRL) == b'\x03'
        assert enc(s, ord('a'), 0, 'a') == b'a'
    finally:
        s.close()


def test_zsh_history_recall():
    s = make_session(['/bin/zsh', '-f', '-i'])  # -f: skip user rc files
    try:
        assert pump_until(s, lambda: '%' in s.text()), s.text()
        s.write_to_child(b'echo phase-c-zsh-marker\r')
        assert pump_until(s, screen_has(s, 'phase-c-zsh-marker')), s.text()
        send_key(s, FKEY_UP)          # recall the echo command from history
        assert pump_until(s, lambda: s.text().count('echo phase-c-zsh-marker') >= 2), s.text()
        send_key(s, FKEY_ENTER)
        assert pump_until(s, lambda: s.text().count('phase-c-zsh-marker') >= 3), s.text()
    finally:
        s.close()


def test_vim_decckm_and_movement():
    with tempfile.NamedTemporaryFile('w', suffix='.txt', delete=False) as f:
        f.write('\n'.join(f'line-{i}' for i in range(1, 6)) + '\n')
        path = f.name
    s = make_session(['/usr/bin/vim', '-u', 'NONE', '-i', 'NONE', path])
    try:
        # vim may pause at a "Press ENTER" hit-enter prompt after the
        # file-info message; dismiss it with an encoded Enter.
        assert pump_until(s, lambda: 'line-1' in s.text() or 'ENTER' in s.text()), s.text()
        if 'ENTER' in s.text():
            send_key(s, FKEY_ENTER)
        assert pump_until(s, screen_has(s, 'line-1')), s.text()
        # vim negotiates keyboard state itself: it enables DECCKM, and modern
        # vim also detects kitty and pushes kitty keyboard-protocol flags.
        # The encoder must follow whatever state vim actually set.
        assert pump_until(s, lambda: s.screen.cursor_key_mode, 4.0)
        flags = s.screen.current_key_encoding_flags()
        expected_up = b'\x1b[A' if flags else b'\x1bOA'  # protocol form vs SS3/DECCKM
        assert enc(s, FKEY_UP) == expected_up, (flags, enc(s, FKEY_UP))
        assert s.screen.cursor.y == 0
        send_key(s, FKEY_DOWN)
        send_key(s, FKEY_DOWN)
        assert pump_until(s, lambda: s.screen.cursor.y == 2), f'cursor at {s.screen.cursor.y}'
        s.write_to_child(b':q!\r')
        assert pump_until(s, lambda: not s.child_alive), 'vim did not exit'
    finally:
        s.close()
        os.unlink(path)


def test_kitty_protocol_flag_stack():
    s = make_session(['/bin/sh', '-c', r"printf '\033[>1u'; sleep 2; printf '\033[<u'; sleep 60"])
    try:
        assert pump_until(s, lambda: s.screen.current_key_encoding_flags() == 1), \
            f'flags={s.screen.current_key_encoding_flags()}'
        # progressive enhancement active: Escape becomes CSI 27 u
        assert enc(s, FKEY_ESCAPE) == b'\x1b[27u'
        assert pump_until(s, lambda: s.screen.current_key_encoding_flags() == 0)
        assert enc(s, FKEY_ESCAPE) == b'\x1b'
    finally:
        s.close()


def test_less_scroll_and_quit():
    with tempfile.NamedTemporaryFile('w', suffix='.txt', delete=False) as f:
        f.write('\n'.join(f'row-{i:03d}' for i in range(1, 101)) + '\n')
        path = f.name
    s = make_session(['/usr/bin/less', path])
    try:
        assert pump_until(s, screen_has(s, 'row-001')), s.text()
        send_key(s, FKEY_DOWN)
        assert pump_until(s, screen_has(s, 'row-012')), s.text()   # 11 visible rows + 1
        send_key(s, FKEY_PAGE_DOWN)
        assert pump_until(s, screen_has(s, 'row-023')), s.text()
        s.write_to_child(enc(s, ord('q'), 0, 'q'))
        assert pump_until(s, lambda: not s.child_alive), 'less did not exit'
    finally:
        s.close()
        os.unlink(path)


@pytest.mark.skipif(shutil.which('tmux') is None, reason='tmux not installed')
def test_tmux_prefix_commands():
    tmux = shutil.which('tmux')
    sock = f'kitmux-test-{os.getpid()}'
    s = make_session([tmux, '-L', sock, '-f', '/dev/null', 'new-session'])
    try:
        assert pump_until(s, screen_has(s, '[0]')), s.text()       # status bar
        # C-b c: new window -> status bar shows window 1 active
        s.write_to_child(enc(s, ord('b'), CTRL))
        s.write_to_child(enc(s, ord('c'), 0, 'c'))
        assert pump_until(s, screen_has(s, '1:')), s.text()
        # C-b d: detach cleanly
        s.write_to_child(enc(s, ord('b'), CTRL))
        s.write_to_child(enc(s, ord('d'), 0, 'd'))
        assert pump_until(s, lambda: not s.child_alive), 'tmux did not detach'
    finally:
        os.system(f'{tmux} -L {sock} kill-server 2>/dev/null')
        s.close()
