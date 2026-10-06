#!/bin/sh
# The bridge presses the paste shortcut in the focused window: Ctrl+V in an
# ordinary app, Ctrl+Shift+V in one the caller lists as a terminal. It gives
# up when no other window takes focus, waits until a window that has just
# taken focus has held it for 100 ms, and refuses callers that are not Pasta.
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

# 4. A window that takes focus while Paste is waiting gets the keys no sooner
#    than 100 ms after it did. Its own focus-in is seen slightly after the
#    shell moved focus, hence the lower bound of 80 ms.
cp "$REPO_ROOT/target/debug/examples/gnome_bridge_paste" "$NEST/writer/pasta-launcher"
"$NEST/writer/pasta-launcher" dev.pasta.FakeTerminal >"$NEST/settle-paste.out" 2>&1 &
PASTE_PID=$!
python3 "$HERE/clip_tool.py" key-catcher dev.pasta.KeyCatcher 15 >"$NEST/settle.log" 2>&1 &
CATCHER_PID=$!
wait "$PASTE_PID" || { echo "FAIL paste said: $(cat "$NEST/settle-paste.out")"; exit 1; }
wait_for_line "$NEST/settle.log" "^KEY ctrl\+v$" 5
focus_us=$(sed -n 's/^AT focus //p' "$NEST/settle.log" | tail -n 1)
key_us=$(sed -n 's/^AT key //p' "$NEST/settle.log" | head -n 1)
gap_ms=$(( (key_us - focus_us) / 1000 ))
[ "$gap_ms" -ge 80 ] || { echo "FAIL keys arrived ${gap_ms} ms after focus; expected at least 80"; exit 1; }
kill "$CATCHER_PID" 2>/dev/null || true
wait "$CATCHER_PID" 2>/dev/null || true

# 5. A caller that is not pasta-launcher is refused.
poke=$(python3 "$HERE/clip_tool.py" poke-paste)
case "$poke" in
    "POKE rejected:"*) ;;
    *) echo "FAIL non-Pasta caller was not rejected: $poke"; exit 1 ;;
esac
wait_for_line "$SHELL_LOG" "pasta-clipboard: rejected Paste from /usr/bin/python3" 5
echo "PASS scenario-paste"
