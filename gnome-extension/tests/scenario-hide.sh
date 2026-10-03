#!/bin/sh
# Pasta's own windows stay out of the dock, Alt+Tab and the overview while the
# extension is enabled; nothing else is hidden, and disabling restores them.
set -eu
. "$HERE/lib.sh"
CHECK="python3 $HERE/probe_check.py"
if [ -d "$NEST/data/gnome-shell/extensions/dash-to-dock@micxgx.gmail.com" ]; then
    busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions EnableExtension s dash-to-dock@micxgx.gmail.com >/dev/null || true
    sleep 2
fi
enable_probe
# Disabling an extension cycles every extension enabled after it, so put the
# probe ahead of the clipboard extension or its counters reset mid-check.
busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions DisableExtension s "$UUID" >/dev/null
busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions EnableExtension s "$UUID" >/dev/null

# An unrelated window that must stay listed throughout.
python3 "$HERE/clip_tool.py" window 120 >"$NEST/window.log" 2>&1 &
OTHER_PID=$!
wait_for_line "$NEST/window.log" "WINDOW shown" 10

start_pasta
trap 'kill "$PASTA_PID" "$OTHER_PID" 2>/dev/null || true' EXIT
"$REPO_ROOT/target/debug/pasta-launcher" --show 2>>"$NEST/pasta.log" || true

# 1 and 2: Pasta hidden everywhere, the other window untouched.
$CHECK wait hidden "$PASTA_PID" 15
$CHECK wait shown "$OTHER_PID" 5
echo "OBSERVED pasta listed in running apps before it was hidden: $($CHECK ever-listed "$PASTA_PID")"

# 5: the third-party dock, when this machine has it.
echo "DASH-TO-DOCK $($CHECK dash-to-dock-lacks "$PASTA_PID")"
$CHECK dash-to-dock-lacks "$PASTA_PID" >/dev/null

# 4: disabling restores Pasta and nudges the lists to redraw.
before=$($CHECK counts "$PASTA_PID")
busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions DisableExtension s "$UUID" >/dev/null
$CHECK wait shown "$PASTA_PID" 10
$CHECK wait nudged "$PASTA_PID" 5 "$before"

# 4: enabling with Pasta already running hides it again, with a nudge.
before=$($CHECK counts "$PASTA_PID")
busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions EnableExtension s "$UUID" >/dev/null
$CHECK wait hidden "$PASTA_PID" 15
$CHECK wait nudged "$PASTA_PID" 5 "$before"
$CHECK wait shown "$OTHER_PID" 5

# 3: a process that is not pasta-launcher gets nothing hidden, even holding Pasta's name.
kill "$PASTA_PID" 2>/dev/null || true
wait "$PASTA_PID" 2>/dev/null || true
python3 "$HERE/clip_tool.py" impostor-window 30 >"$NEST/impostor-window.log" 2>&1 &
IMPOSTOR_PID=$!
trap 'kill "$IMPOSTOR_PID" "$OTHER_PID" 2>/dev/null || true' EXIT
wait_for_line "$NEST/impostor-window.log" "IMPOSTOR owns com.pasta.Launcher" 10
wait_for_line "$NEST/impostor-window.log" "WINDOW shown" 10
wait_for_line "$SHELL_LOG" "pasta-clipboard: not hiding: com.pasta.Launcher is owned by /usr/bin/python3" 10
$CHECK wait shown "$IMPOSTOR_PID" 10
echo "PASS scenario-hide"
