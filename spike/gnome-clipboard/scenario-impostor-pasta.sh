#!/bin/sh
# With a non-Pasta process holding com.pasta.Launcher, the extension sends it nothing.
set -eu
. "$HERE/lib.sh"
python3 "$HERE/clip_tool.py" impostor pasta 12 >"$NEST/impostor.log" 2>&1 &
wait_for_line "$NEST/impostor.log" "IMPOSTOR owns com.pasta.Launcher" 10
python3 "$HERE/clip_tool.py" set-text "for pasta only"
wait_for_line "$SPIKE_LOG" "PASTA-SPIKE refusing to offer: com.pasta.Launcher is owned by /usr/bin/python3" 10
refute_line "$NEST/impostor.log" "IMPOSTOR got"
echo "PASS scenario-impostor-pasta"
