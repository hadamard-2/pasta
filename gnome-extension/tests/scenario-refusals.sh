#!/bin/sh
# Each side refuses peers that are not who they claim to be.
set -eu
. "$HERE/lib.sh"

# 1. The extension offers nothing to an impostor holding Pasta's name.
python3 "$HERE/clip_tool.py" impostor pasta 10 >"$NEST/impostor-pasta.log" 2>&1 &
IMPOSTOR_PID=$!
wait_for_line "$NEST/impostor-pasta.log" "IMPOSTOR owns com.pasta.Launcher" 10
python3 "$HERE/clip_tool.py" set-text "for pasta only"
wait_for_line "$SHELL_LOG" "pasta-clipboard: not offering: com.pasta.Launcher is owned by /usr/bin/python3" 10
kill "$IMPOSTOR_PID" 2>/dev/null || true
wait "$IMPOSTOR_PID" 2>/dev/null || true
refute_line "$NEST/impostor-pasta.log" "IMPOSTOR got"

# 2. Pasta refuses an Offer from anything but /usr/bin/gnome-shell.
start_pasta
if busctl --user call com.pasta.Launcher /com/pasta/Launcher/Clipboard \
        com.pasta.Launcher.Clipboard1 Offer as 1 text/plain >/dev/null 2>&1; then
    echo "FAIL busctl Offer was accepted"
    exit 1
fi
wait_for_line "$NEST/pasta.log" "refused GNOME clipboard Offer from /usr/bin/busctl" 5

# 3. The bridge refuses a caller that is not pasta-launcher.
poke=$(python3 "$HERE/clip_tool.py" poke-bridge)
case "$poke" in
    "POKE rejected:"*) ;;
    *) echo "FAIL non-Pasta caller was not rejected: $poke"; exit 1 ;;
esac
wait_for_line "$SHELL_LOG" "pasta-clipboard: rejected SetClipboard from /usr/bin/python3" 5

# 4. Pasta's client refuses to write to an impostor bridge.
busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions DisableExtension s "$UUID" >/dev/null
python3 "$HERE/clip_tool.py" impostor bridge 10 >"$NEST/impostor-bridge.log" 2>&1 &
wait_for_line "$NEST/impostor-bridge.log" "IMPOSTOR owns com.pasta.Launcher.ShellBridge" 10
printf 'secret' >"$NEST/secret.txt"
if write_as_pasta "text/plain;charset=utf-8" "$NEST/secret.txt" 2>"$NEST/writer.err"; then
    echo "FAIL write to an impostor bridge succeeded"
    exit 1
fi
grep -q "is owned by /usr/bin/python3" "$NEST/writer.err" || { echo "FAIL unexpected writer error: $(cat "$NEST/writer.err")"; exit 1; }
sleep 1
refute_line "$NEST/impostor-bridge.log" "IMPOSTOR got"
echo "PASS scenario-refusals"
