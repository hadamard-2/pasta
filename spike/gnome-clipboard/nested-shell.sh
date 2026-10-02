#!/bin/sh
# Runs one spike scenario inside a throwaway, headless GNOME Shell.
#
# Isolation: a private D-Bus session (dbus-run-session), a private Wayland
# display, XDG config/data/cache/state under $NEST, and the in-memory
# GSettings backend. Nothing here can reach the live desktop session or its
# settings. Never load the spike extension into the live shell: an exception
# there can end the whole Wayland session.
#
# Usage: nested-shell.sh <scenario-script> [args...]
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
UUID=pasta-clipboard-spike@pasta.launcher
NEST=${NEST:-$(mktemp -d "${TMPDIR:-/tmp}/pasta-spike.XXXXXX")}
mkdir -p "$NEST/config" "$NEST/cache" "$NEST/state" "$NEST/bin" "$NEST/data/gnome-shell/extensions"
# GNOME Shell discovers extensions only at start-up, so the extension must be
# in place before the shell launches.
rm -rf "$NEST/data/gnome-shell/extensions/$UUID"
cp -r "$HERE/$UUID" "$NEST/data/gnome-shell/extensions/"
export XDG_CONFIG_HOME="$NEST/config" XDG_DATA_HOME="$NEST/data" \
    XDG_CACHE_HOME="$NEST/cache" XDG_STATE_HOME="$NEST/state" \
    GSETTINGS_BACKEND=memory NEST HERE UUID
echo "nest: $NEST"
exec dbus-run-session -- "$HERE/nested-inner.sh" "$@"
