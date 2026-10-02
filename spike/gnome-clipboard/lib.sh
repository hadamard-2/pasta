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
