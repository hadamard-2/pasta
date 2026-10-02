#!/bin/sh
# Text, a ~3 MB PNG and a ~15 MB incompressible PNG reach Pasta intact.
set -eu
. "$HERE/lib.sh"
start_pasta

python3 "$HERE/clip_tool.py" set-text "spike text"
wait_for_line "$NEST/pasta.log" "RECEIVED text/plain;charset=utf-8 10 bytes .*sha256=$(printf 'spike text' | sha256sum | cut -c1-12)" 10

python3 "$HERE/clip_tool.py" make-png "$NEST/mid.png" 2880 1800 5
python3 "$HERE/clip_tool.py" make-png "$NEST/noise.png" 2880 1800 1
for png in mid noise; do
    size=$(stat -c %s "$NEST/$png.png")
    sha=$(sha256sum "$NEST/$png.png" | cut -c1-12)
    python3 "$HERE/clip_tool.py" set-file image/png "$NEST/$png.png"
    wait_for_line "$NEST/pasta.log" "RECEIVED image/png $size bytes in [0-9]+ ms sha256=$sha\$" 15
done

refute_line "$NEST/pasta.log" "TRUNCATED|failed"
echo "--- pasta.log"; cat "$NEST/pasta.log"
echo "--- extension"; grep -a "PASTA-SPIKE" "$SPIKE_LOG" | sed 's/^.*PASTA-SPIKE/PASTA-SPIKE/'
echo "PASS scenario-offer"
