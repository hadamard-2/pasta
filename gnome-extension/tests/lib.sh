# Shared helpers for scenario scripts. Sourced, not executed.

# wait_for_line <file> <ERE> <timeout-seconds>
wait_for_line() {
    deadline=$(( $(date +%s) + $3 ))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        if [ -f "$1" ] && grep -aqE -- "$2" "$1"; then
            return 0
        fi
        sleep 0.2
    done
    echo "FAIL waited $3s in $1 for: $2"
    return 1
}

# refute_line <file> <ERE>
refute_line() {
    if [ -f "$1" ] && grep -aqE -- "$2" "$1"; then
        echo "FAIL unexpected line in $1: $(grep -aE -- "$2" "$1" | head -1)"
        return 1
    fi
    return 0
}

# start_pasta: run the real pasta-launcher inside the nested session; its
# stderr goes to $NEST/pasta.log.
start_pasta() {
    "$REPO_ROOT/target/debug/pasta-launcher" 2>>"$NEST/pasta.log" &
    PASTA_PID=$!
    trap 'kill "$PASTA_PID" 2>/dev/null || true' EXIT
    wait_for_line "$NEST/pasta.log" "Linux launcher window created" 30
}

# write_as_pasta <mimetype> <file>: send through the bridge using Pasta's real
# client, under the executable name the bridge serves.
write_as_pasta() {
    cp "$REPO_ROOT/target/debug/examples/gnome_bridge_write" "$NEST/writer/pasta-launcher"
    "$NEST/writer/pasta-launcher" "$1" "$2"
}
