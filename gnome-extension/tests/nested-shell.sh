#!/bin/sh
# Runs one scenario inside a throwaway, headless GNOME Shell with the real
# clipboard@pasta.launcher extension installed.
#
# Isolation: a private D-Bus session (dbus-run-session), a private Wayland
# display, XDG config/data/cache/state under $NEST, and the in-memory
# GSettings backend. The runtime directory is shared with the desktop
# (gnome-shell will not start without it), which is why a live Pasta must not
# be running: its single-instance lock would capture the test launch.
#
# Usage: nested-shell.sh <scenario-script> [args...]
set -eu
if pgrep -x pasta-launcher >/dev/null 2>&1; then
    echo "FAIL a pasta-launcher is running; quit it before running the GNOME extension tests"
    exit 1
fi
HERE=$(cd "$(dirname "$0")" && pwd)
REPO_ROOT=$(cd "$HERE/../.." && pwd)
UUID=clipboard@pasta.launcher
NEST=${NEST:-$(mktemp -d "${TMPDIR:-/tmp}/pasta-gnome-test.XXXXXX")}
mkdir -p "$NEST/config" "$NEST/cache" "$NEST/state" "$NEST/writer" "$NEST/data/gnome-shell/extensions"
# GNOME Shell discovers extensions only at start-up.
rm -rf "$NEST/data/gnome-shell/extensions/$UUID"
cp -r "$REPO_ROOT/gnome-extension/$UUID" "$NEST/data/gnome-shell/extensions/"
# Test-only probe; scenarios that need it enable it themselves.
rm -rf "$NEST/data/gnome-shell/extensions/probe@pasta.launcher"
cp -r "$HERE/probe@pasta.launcher" "$NEST/data/gnome-shell/extensions/"
export XDG_CONFIG_HOME="$NEST/config" XDG_DATA_HOME="$NEST/data" \
    XDG_CACHE_HOME="$NEST/cache" XDG_STATE_HOME="$NEST/state" \
    GSETTINGS_BACKEND=memory NEST HERE REPO_ROOT UUID
echo "nest: $NEST"
exec dbus-run-session -- "$HERE/nested-inner.sh" "$@"
