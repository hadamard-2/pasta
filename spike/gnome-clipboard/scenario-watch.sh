#!/bin/sh
# A Wayland client's copy reaches the extension.
set -eu
. "$HERE/lib.sh"
wait_for_line "$SPIKE_LOG" "PASTA-SPIKE enabled" 10
python3 "$HERE/clip_tool.py" set-text "spike text"
wait_for_line "$SPIKE_LOG" 'PASTA-SPIKE owner-changed .*mimetypes=\[.*"text/plain' 10
grep -a "PASTA-SPIKE owner-changed" "$SPIKE_LOG" | sed 's/^.*PASTA-SPIKE/PASTA-SPIKE/'
echo "PASS scenario-watch"
