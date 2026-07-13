#!/usr/bin/env bash
# Throttled RAM/tmp sanity check for tcui agent sessions.
#
# Self-throttles to max once per 30 min via a timestamp file.
# Safe to call frequently; it skips if called again too soon.
#
# Default: quiet — only prints warnings when something is wrong.
#   ./scripts/preflight.sh           # throttled, quiet (warnings only)
#   ./scripts/preflight.sh --verbose # full report even when clean
#   ./scripts/preflight.sh --force   # bypass throttle, always runs
#   ./scripts/preflight.sh --status  # only report state, no cleanup
#   ./scripts/preflight.sh -q        # force quiet (suppress throttle msg too)
#
# Does NOT block — always exits 0. Warnings are advisory.

set -euo pipefail

THROTTLE_MINUTES="${PREFLIGHT_THROTTLE_MIN:-30}"
STALE_MINUTES="${PREFLIGHT_STALE_MIN:-30}"
TIMESTAMP_FILE="${PREFLIGHT_TS:-.omo/preflight.last}"

mode="run"
quiet=false
for arg in "$@"; do
    case "$arg" in
        --force) mode="force" ;;
        --status) mode="status" ;;
        --verbose|-v) quiet=false ;;
        -q|--quiet) quiet=true ;;
        -h|--help)
            sed -n '2,15p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
    esac
done

# --- Throttle ---
if [[ "$mode" != "force" ]]; then
    if [[ -f "$TIMESTAMP_FILE" ]]; then
        last=$(stat -c %Y "$TIMESTAMP_FILE" 2>/dev/null || echo 0)
        now=$(date +%s)
        elapsed=$(( (now - last) / 60 ))
        if [[ $elapsed -lt $THROTTLE_MINUTES ]]; then
            remaining=$(( THROTTLE_MINUTES - elapsed ))
            [[ "$quiet" == "false" ]] && echo "preflight: throttled (${elapsed}m ago; ${remaining}m until next; --force to bypass)"
            exit 0
        fi
    fi
fi

# --- Run ---
if [[ "$mode" == "run" ]]; then
    mkdir -p .omo
    date +%s > "$TIMESTAMP_FILE"
fi

# --- Clean stale /tmp/tcui-* dirs ---
cleaned=0
if [[ "$mode" != "status" ]]; then
    while IFS= read -r dir; do
        [[ -z "$dir" ]] && continue
        if rm -rf "$dir" 2>/dev/null; then
            cleaned=$(( cleaned + 1 ))
        fi
    done < <(find /tmp -maxdepth 1 -name 'tcui-*' -type d -mmin +"$STALE_MINUTES" 2>/dev/null || true)
fi

remaining=$(find /tmp -maxdepth 1 -name 'tcui-*' -type d 2>/dev/null | wc -l)
tmp_size="0"
if compgen -G "/tmp/tcui-*" > /dev/null 2>&1; then
    tmp_size=$(du -sh /tmp/tcui-* 2>/dev/null | tail -1 | awk '{print $1}')
fi

# --- Memory ---
mem_total=$(awk '/^MemTotal:/ {printf "%.0f", $2/1024/1024}' /proc/meminfo)
mem_avail=$(awk '/^MemAvailable:/ {printf "%.0f", $2/1024/1024}' /proc/meminfo)
mem_used=$(( mem_total - mem_avail ))

swap_total=$(awk '/^SwapTotal:/ {printf "%.0f", $2/1024/1024}' /proc/meminfo)
swap_free=$(awk '/^SwapFree:/ {printf "%.0f", $2/1024/1024}' /proc/meminfo)
swap_used=$(( swap_total - swap_free ))
swap_pct=0
[[ $swap_total -gt 0 ]] && swap_pct=$(( swap_used * 100 / swap_total ))

# --- Verbose report (only when requested) ---
if [[ "$quiet" == "false" && "$mode" != "status" ]]; then
    echo "preflight: cleaned=${cleaned} remaining=${remaining} tmp=${tmp_size:-0} ram=${mem_avail}G avail swap=${swap_pct}%"
fi

# --- Warnings (always shown, unless -q) ---
warnings=0
if [[ $mem_avail -lt 4 ]]; then
    echo "preflight: WARN low RAM avail=${mem_avail}G (threshold 4G) — consider closing heavy apps"
    warnings=$(( warnings + 1 ))
fi
if [[ $swap_pct -gt 75 ]]; then
    echo "preflight: WARN swap ${swap_pct}% used (threshold 75%) — memory pressure"
    warnings=$(( warnings + 1 ))
fi
if [[ $remaining -gt 50 ]]; then
    echo "preflight: WARN ${remaining} tcui dirs in /tmp — run preflight with --force to clean"
    warnings=$(( warnings + 1 ))
fi

# --- If no warnings and not verbose, stay quiet ---
if [[ $warnings -eq 0 && "$quiet" == "false" && "$mode" != "status" ]]; then
    echo "preflight: OK (ram=${mem_avail}G avail swap=${swap_pct}% tmp=${remaining} dirs)"
fi

exit 0
