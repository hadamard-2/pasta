#!/bin/sh
# Runs every scenario, each in its own fresh nested shell.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
cargo build --example gnome_bridge_spike --manifest-path "$HERE/../../Cargo.toml" || exit 1
failed=0
for scenario in watch reject-caller offer impostor-pasta write; do
    if "$HERE/nested-shell.sh" "$HERE/scenario-$scenario.sh"; then
        :
    else
        echo "FAILED scenario-$scenario"
        failed=1
    fi
done
exit "$failed"
