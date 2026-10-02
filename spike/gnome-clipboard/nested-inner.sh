#!/bin/sh
# Runs inside the private D-Bus session created by nested-shell.sh.
set -eu
DISPLAY_NAME=pasta-spike-$$
export SPIKE_LOG="$NEST/shell.log"
gnome-shell --headless --wayland --no-x11 --virtual-monitor 1280x800 \
    --wayland-display "$DISPLAY_NAME" >"$SPIKE_LOG" 2>&1 &
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

export WAYLAND_DISPLAY="$DISPLAY_NAME" GDK_BACKEND=wayland
"$@"
