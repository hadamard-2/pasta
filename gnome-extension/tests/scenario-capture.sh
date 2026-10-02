#!/bin/sh
# Text, an image and a file-manager reference reach Pasta's snapshot.
set -eu
. "$HERE/lib.sh"
start_pasta

python3 "$HERE/clip_tool.py" set-text "harness text"
wait_for_line "$NEST/pasta.log" 'snapshot published: .*payloads=\[.*text/plain;charset=utf-8=12' 15

python3 "$HERE/clip_tool.py" make-png "$NEST/image.png" 1440 900
size=$(stat -c %s "$NEST/image.png")
python3 "$HERE/clip_tool.py" set-file image/png "$NEST/image.png"
wait_for_line "$NEST/pasta.log" "snapshot published: .*payloads=\[image/png=$size" 15

python3 "$HERE/clip_tool.py" set-file-reference "$NEST/image.png"
wait_for_line "$NEST/pasta.log" 'snapshot published: .*x-special/gnome-copied-files=' 15

refute_line "$NEST/pasta.log" "payload .* dropped|refused GNOME clipboard Offer"
refute_line "$SHELL_LOG" "pasta-clipboard: (offer failed|transfer of)|JS ERROR"
echo "PASS scenario-capture"
