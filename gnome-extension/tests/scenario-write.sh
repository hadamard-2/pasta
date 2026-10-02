#!/bin/sh
# Pasta's real client writes text and an image through the bridge; the shell
# then owns the clipboard and offers it straight back.
set -eu
. "$HERE/lib.sh"
start_pasta

printf 'written by pasta' >"$NEST/text.txt"
python3 "$HERE/clip_tool.py" make-png "$NEST/write.png" 800 600
for case in "text/plain;charset=utf-8 text.txt" "image/png write.png"; do
    set -- $case
    size=$(stat -c %s "$NEST/$2")
    out=$(write_as_pasta "$1" "$NEST/$2")
    [ "$out" = "WROTE $1 $size" ] || { echo "FAIL write said: $out"; exit 1; }
    wait_for_line "$NEST/pasta.log" "snapshot published: .*$1=$size" 15
done
echo "PASS scenario-write"
