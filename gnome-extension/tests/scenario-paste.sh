#!/bin/sh
# The bridge presses the paste shortcut in the focused window: Ctrl+V in an
# ordinary app, Ctrl+Shift+V in one the caller lists as a terminal. It gives
# up when no other window takes focus and refuses callers that are not Pasta.
set -eu
. "$HERE/lib.sh"

paste_as_pasta() {
    cp "$REPO_ROOT/target/debug/examples/gnome_bridge_paste" "$NEST/writer/pasta-launcher"
    "$NEST/writer/pasta-launcher" "$@"
}

# The probe gives the headless seat a keyboard, as a real session has, before
# any client starts.
enable_probe

# 1. Nothing to paste into: no window is focused.
if paste_as_pasta dev.pasta.FakeTerminal 2>"$NEST/paste-none.err"; then
    echo "FAIL Paste succeeded with no window to paste into"
    exit 1
fi
grep -q "no window other than the caller's took focus within 1000 ms" "$NEST/paste-none.err" \
    || { echo "FAIL unexpected error: $(cat "$NEST/paste-none.err")"; exit 1; }

# 2. An ordinary app gets Ctrl+V.
python3 "$HERE/clip_tool.py" key-catcher dev.pasta.KeyCatcher 15 >"$NEST/catcher.log" 2>&1 &
CATCHER_PID=$!
wait_for_line "$NEST/catcher.log" "WINDOW shown" 10
out=$(paste_as_pasta dev.pasta.FakeTerminal)
[ "$out" = "PASTED" ] || { echo "FAIL paste said: $out"; exit 1; }
wait_for_line "$NEST/catcher.log" "^KEY ctrl\+v$" 5
refute_line "$NEST/catcher.log" "^KEY ctrl\+shift\+v$"
kill "$CATCHER_PID" 2>/dev/null || true
wait "$CATCHER_PID" 2>/dev/null || true

# 3. A listed terminal gets Ctrl+Shift+V.
python3 "$HERE/clip_tool.py" key-catcher dev.pasta.FakeTerminal 15 >"$NEST/terminal.log" 2>&1 &
CATCHER_PID=$!
wait_for_line "$NEST/terminal.log" "WINDOW shown" 10
out=$(paste_as_pasta dev.pasta.FakeTerminal)
[ "$out" = "PASTED" ] || { echo "FAIL paste said: $out"; exit 1; }
wait_for_line "$NEST/terminal.log" "^KEY ctrl\+shift\+v$" 5
refute_line "$NEST/terminal.log" "^KEY ctrl\+v$"
kill "$CATCHER_PID" 2>/dev/null || true
wait "$CATCHER_PID" 2>/dev/null || true

# 4. A caller that is not pasta-launcher is refused.
poke=$(python3 "$HERE/clip_tool.py" poke-paste)
case "$poke" in
    "POKE rejected:"*) ;;
    *) echo "FAIL non-Pasta caller was not rejected: $poke"; exit 1 ;;
esac
wait_for_line "$SHELL_LOG" "pasta-clipboard: rejected Paste from /usr/bin/python3" 5
echo "PASS scenario-paste"
