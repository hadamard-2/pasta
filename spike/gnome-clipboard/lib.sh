# Shared helpers for scenario scripts. Sourced, not executed.

# wait_for_line <file> <grep-pattern> <timeout-seconds>
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

# refute_line <file> <grep-pattern>
refute_line() {
    if [ -f "$1" ] && grep -aqE -- "$2" "$1"; then
        echo "FAIL unexpected line in $1: $(grep -aE -- "$2" "$1" | head -1)"
        return 1
    fi
    return 0
}

# start_pasta: run the spike service under the executable name the extension
# trusts. Stderr goes to $NEST/pasta.log.
REPO_ROOT=$(cd "$HERE/../.." && pwd)
start_pasta() {
    cp "$REPO_ROOT/target/debug/examples/gnome_bridge_spike" "$NEST/bin/pasta-launcher"
    "$NEST/bin/pasta-launcher" serve 2>>"$NEST/pasta.log" &
    PASTA_PID=$!
    trap 'kill "$PASTA_PID" 2>/dev/null || true' EXIT
    wait_for_line "$NEST/pasta.log" "serving com.pasta.Launcher" 10
}
