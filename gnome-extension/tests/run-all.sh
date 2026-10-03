#!/bin/sh
# Builds Pasta and the write test client, then runs every scenario, each in
# its own fresh isolated GNOME Shell. Local only: needs GNOME Shell 50.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
cargo build --manifest-path "$HERE/../../Cargo.toml" --bin pasta-launcher --example gnome_bridge_write || exit 1
failed=0
for scenario in capture startup write refusals hide; do
    if "$HERE/nested-shell.sh" "$HERE/scenario-$scenario.sh"; then
        :
    else
        echo "FAILED scenario-$scenario"
        failed=1
    fi
done
exit "$failed"
