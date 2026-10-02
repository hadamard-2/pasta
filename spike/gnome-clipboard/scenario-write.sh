#!/bin/sh
# Pasta can write text and an image through the bridge; the write echoes back
# through Offer with the same hash; non-Pasta callers and impostor bridges are refused.
set -eu
. "$HERE/lib.sh"
start_pasta

printf 'written by pasta' >"$NEST/text.txt"
python3 "$HERE/clip_tool.py" make-png "$NEST/mid.png" 2880 1800 5
for case in "text/plain;charset=utf-8 text.txt" "image/png mid.png"; do
    set -- $case
    sha=$(sha256sum "$NEST/$2" | cut -c1-12)
    "$NEST/bin/pasta-launcher" write "$1" "$NEST/$2" 2>>"$NEST/pasta.log"
    wait_for_line "$NEST/pasta.log" "WROTE $1 .*sha256=$sha" 5
    wait_for_line "$SPIKE_LOG" "PASTA-SPIKE SetClipboard $1 .*sha256=$sha" 5
    # The shell now owns the clipboard with our bytes; owner-changed fires and
    # the extension offers them straight back.
    wait_for_line "$NEST/pasta.log" "RECEIVED $1 .*sha256=$sha" 10
done

poke=$(python3 "$HERE/clip_tool.py" poke-bridge)
echo "$poke"
case "$poke" in
    "POKE rejected:"*) ;;
    *) echo "FAIL non-Pasta caller was not rejected"; exit 1 ;;
esac
wait_for_line "$SPIKE_LOG" "PASTA-SPIKE rejected SetClipboard from /usr/bin/python3" 5

# Free the bridge name, let an impostor take it, and confirm Pasta will not write to it.
busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions DisableExtension s "$UUID" >/dev/null
python3 "$HERE/clip_tool.py" impostor bridge 10 >"$NEST/impostor.log" 2>&1 &
wait_for_line "$NEST/impostor.log" "IMPOSTOR owns com.pasta.Launcher.ShellBridge" 10
if "$NEST/bin/pasta-launcher" write "text/plain;charset=utf-8" "$NEST/text.txt" 2>>"$NEST/pasta.log"; then
    echo "FAIL write to an impostor bridge succeeded"
    exit 1
fi
wait_for_line "$NEST/pasta.log" "refusing to write: com.pasta.Launcher.ShellBridge is owned by /usr/bin/python3" 5
refute_line "$NEST/impostor.log" "IMPOSTOR got"
echo "PASS scenario-write"
