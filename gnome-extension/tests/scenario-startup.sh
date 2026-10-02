#!/bin/sh
# Something copied before Pasta starts is offered once Pasta appears.
set -eu
. "$HERE/lib.sh"
python3 "$HERE/clip_tool.py" set-text "before pasta"
start_pasta
wait_for_line "$NEST/pasta.log" 'snapshot published: .*payloads=\[.*text/plain;charset=utf-8=12' 15
echo "PASS scenario-startup"
