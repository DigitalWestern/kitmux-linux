#!/usr/bin/env python3
"""Generate the Linux form of the locked macOS libkitty session suite."""

from pathlib import Path
import sys


source_path = Path(sys.argv[1])
destination = Path(sys.argv[2])
source = source_path.read_text().replace("/private/tmp", "/tmp")

# The tagged macOS test checks Kitmux's replacement for Apple's exact stock
# zsh prompt. Linux distributions ship different defaults, so this is not a
# portable libkitty contract. Preserve the cwd test cleanup and every test
# that follows it.
start_marker = "    char zshrc_path[PATH_MAX];\n"
end_marker = "    rmdir(test_cwd);\n"
start = source.find(start_marker)
end = source.find(end_marker, start)
if start < 0 or end < 0:
    raise SystemExit("locked zsh prompt test block changed")
end += len(end_marker)
source = (
    source[:start]
    + "    // macOS-only stock-zsh-prompt policy intentionally omitted.\n"
    + end_marker
    + source[end:]
)

# The portable text-extraction contract also names emoji explicitly. Keep the
# locked wide/combining case and add one exact emoji selection assertion to its
# generated Linux form.
unicode_input = '        "printf \'A界éZ\'; read x", NULL};\n'
unicode_input_with_emoji = '        "printf \'A界é🙂Z\'; read x", NULL};\n'
unicode_assertion = '''    assert(selected && strcmp(selected, "界é") == 0);
    free(selected);
'''
unicode_assertion_with_emoji = unicode_assertion + '''    kitty_session_selection_start(unicode_session, 4, 0, true, 0);
    kitty_session_selection_update(unicode_session, 5, 0, false, true);
    selected = kitty_session_selection_text(unicode_session);
    assert(selected && strcmp(selected, "🙂") == 0);
    free(selected);
'''
if unicode_input not in source or unicode_assertion not in source:
    raise SystemExit("locked Unicode extraction test block changed")
source = source.replace(unicode_input, unicode_input_with_emoji, 1)
source = source.replace(unicode_assertion, unicode_assertion_with_emoji, 1)

destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_text(source)
print(f"generated {destination}")
