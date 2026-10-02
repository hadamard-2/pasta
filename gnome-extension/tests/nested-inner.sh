#!/bin/sh
# Runs inside the private D-Bus session created by nested-shell.sh.
set -eu
DISPLAY_NAME=pasta-gnome-test-$$
export SHELL_LOG="$NEST/shell.log"
gnome-shell --headless --wayland --no-x11 --virtual-monitor 1280x800 \
    --wayland-display "$DISPLAY_NAME" >"$SHELL_LOG" 2>&1 &
SHELL_PID=$!
trap 'kill "$SHELL_PID" 2>/dev/null; wait "$SHELL_PID" 2>/dev/null || true' EXIT

ready=no
for _ in $(seq 1 80); do
    if busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions ListExtensions >/dev/null 2>&1; then
        ready=yes
        break
    fi
    sleep 0.25
done
[ "$ready" = yes ] || { echo "FAIL nested shell never exposed org.gnome.Shell.Extensions"; exit 1; }

enabled=$(busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions EnableExtension s "$UUID")
[ "$enabled" = "b true" ] || { echo "FAIL EnableExtension returned: $enabled"; exit 1; }

# EnableExtension returns before the extension has loaded; GetExtensionInfo
# omits "state" until it has. Wait for state 1 (ENABLED).
loaded=no
for _ in $(seq 1 40); do
    if busctl --user call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions GetExtensionInfo s "$UUID" | grep -q '"state" d 1'; then
        loaded=yes
        break
    fi
    sleep 0.25
done
[ "$loaded" = yes ] || { echo "FAIL $UUID never reached state 1 (ENABLED)"; exit 1; }

export WAYLAND_DISPLAY="$DISPLAY_NAME" GDK_BACKEND=wayland XDG_CURRENT_DESKTOP=GNOME

# On a freshly started shell the first client's clipboard set is silently
# dropped by the compositor. Probe until a copy reads back, so no scenario's
# first copy is the one that gets lost.
ready=no
for n in 1 2 3 4 5; do
    python3 "$HERE/clip_tool.py" set-text "pasta-harness-probe-$n" &
    probe_pid=$!
    sleep 1.5
    got=$(python3 "$HERE/clip_tool.py" get-text 2>/dev/null || true)
    wait "$probe_pid" || true
    if [ "$got" = "pasta-harness-probe-$n" ]; then
        ready=yes
        break
    fi
done
[ "$ready" = yes ] || { echo "FAIL nested shell never accepted a clipboard copy"; exit 1; }
"$@"
