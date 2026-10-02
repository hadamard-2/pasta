#!/bin/sh
# Pasta refuses Offer from anything that is not /usr/bin/gnome-shell.
set -eu
. "$HERE/lib.sh"
start_pasta
if out=$(busctl --user call com.pasta.Launcher /com/pasta/Launcher/Clipboard \
        com.pasta.Launcher.Clipboard1 Offer as 1 text/plain 2>&1); then
    echo "FAIL busctl Offer was accepted: $out"
    exit 1
fi
echo "busctl: $out"
case "$out" in
    *"Access denied"*) ;;  # busctl maps AccessDenied to its own text
    *) echo "FAIL unexpected error text"; exit 1 ;;
esac
wait_for_line "$NEST/pasta.log" "rejected Offer from /usr/bin/busctl" 5
echo "PASS scenario-reject-caller"
