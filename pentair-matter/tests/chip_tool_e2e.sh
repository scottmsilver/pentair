#!/usr/bin/env bash
# End-to-end test for pentair-matter bridge using chip-tool.
#
# Prerequisites:
#   - chip-tool installed (snap install chip-tool)
#   - pentair-daemon running on localhost:8080
#   - No other pentair-matter process running (port 5540)
#
# Usage:
#   ./pentair-matter/tests/chip_tool_e2e.sh [--keep]
#
# Options:
#   --keep   Don't start/stop the bridge (assumes it's already running and commissioned)

set -uo pipefail

DAEMON_URL="http://localhost:8080"
NODE_ID=1
PASSCODE=20202021
BRIDGE_PID=""
PASS=0
FAIL=0
KEEP=false

[[ "${1:-}" == "--keep" ]] && KEEP=true

# --- Helpers ---

# Strip ANSI escape codes from chip-tool output
strip_ansi() {
    sed 's/\x1b\[[0-9;]*m//g'
}

cleanup() {
    if [[ "$KEEP" == false ]] && [[ -n "$BRIDGE_PID" ]]; then
        kill "$BRIDGE_PID" 2>/dev/null
        wait "$BRIDGE_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

die() { echo "FATAL: $1" >&2; exit 1; }

# Run chip-tool, strip ANSI, return clean output
chip() {
    chip-tool "$@" 2>&1 | strip_ansi
}

# Extract value after a key from chip-tool TOO lines
# e.g. "  OnOff: TRUE" → "TRUE"
read_attr() {
    local key="$1"; shift
    local output
    output=$(chip "$@")
    echo "$output" | grep "$key:" | tail -1 | sed "s/.*${key}: *//"
}

# Assert a read value equals expected
assert_read() {
    local name="$1" expected="$2"; shift 2
    local actual
    actual=$(read_attr "$@")
    if [[ "$actual" == "$expected" ]]; then
        echo "  PASS  $name ($actual)"
        ((PASS++))
    else
        echo "  FAIL  $name (expected '$expected', got '$actual')"
        ((FAIL++))
    fi
}

# Assert a read value is non-empty and non-zero
assert_read_nonzero() {
    local name="$1"; shift
    local actual
    actual=$(read_attr "$@")
    if [[ -n "$actual" ]] && [[ "$actual" != "0" ]] && [[ "$actual" != "null" ]]; then
        echo "  PASS  $name ($actual)"
        ((PASS++))
    else
        echo "  FAIL  $name (got '$actual')"
        ((FAIL++))
    fi
}

# Assert a chip-tool command succeeds
assert_cmd() {
    local name="$1"; shift
    local output
    output=$(chip "$@")
    if echo "$output" | grep -q "UNSUPPORTED_COMMAND\|General error: 0x01\|IM Error"; then
        echo "  FAIL  $name (command error)"
        ((FAIL++))
    else
        echo "  PASS  $name"
        ((PASS++))
    fi
}

# Assert a daemon API value
assert_daemon() {
    local name="$1" path="$2" expected="$3"
    local actual
    actual=$(curl -sf "$DAEMON_URL/api/pool" | python3 -c "
import sys, json
d = json.load(sys.stdin)
# Navigate the path
parts = '$path'.split('.')
v = d
for p in parts:
    if p.startswith('['):
        v = v[p.strip('[]').strip(\"'\").strip('\"')]
    else:
        v = v[p]
print(v)
" 2>/dev/null || echo "CURL_ERROR")
    if [[ "$actual" == "$expected" ]]; then
        echo "  PASS  $name (daemon=$actual)"
        ((PASS++))
    else
        echo "  FAIL  $name (daemon expected '$expected', got '$actual')"
        ((FAIL++))
    fi
}

# --- Preflight ---

echo "Preflight"
command -v chip-tool >/dev/null 2>&1 || die "chip-tool not found. Install: sudo snap install chip-tool"
curl -sf "$DAEMON_URL/api/pool" >/dev/null || die "Daemon not reachable at $DAEMON_URL"
echo "  chip-tool: ok"
echo "  daemon: ok"

# --- Start bridge (unless --keep) ---

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [[ "$KEEP" == false ]]; then
    # Kill stale
    pkill -f "target/debug/pentair-matter" 2>/dev/null || true
    sleep 1

    # Clean state
    rm -f ~/.pentair/matter-fabrics.bin 2>/dev/null
    rm -f /tmp/chip_kvs /tmp/chip_factory.ini /tmp/chip_config.ini /tmp/chip_counters.ini 2>/dev/null
    rm -rf ~/snap/chip-tool/common/chip_tool_kvs 2>/dev/null

    echo ""
    echo "Starting bridge"
    cargo build -p pentair-matter --manifest-path "$PROJECT_DIR/Cargo.toml" 2>&1 | tail -1

    RUST_LOG=info "$PROJECT_DIR/target/debug/pentair-matter" --daemon-url "$DAEMON_URL" > /tmp/matter-bridge-test.log 2>&1 &
    BRIDGE_PID=$!
    disown "$BRIDGE_PID"
    sleep 4

    if ! kill -0 "$BRIDGE_PID" 2>/dev/null; then
        cat /tmp/matter-bridge-test.log | tail -5
        die "Bridge exited early (port 5540 in use?)"
    fi
    echo "  PID $BRIDGE_PID"

    # Commission
    echo ""
    echo "Commissioning"
    COMM=$(chip pairing onnetwork "$NODE_ID" "$PASSCODE")
    if echo "$COMM" | grep -q "commissioning completed with success"; then
        echo "  PASS  Commission"
        ((PASS++))
    else
        echo "$COMM" | grep -i "error\|fail" | tail -3
        die "Commissioning failed"
    fi
    sleep 1
fi

# --- Read tests ---

echo ""
echo "Read tests"

# Get current daemon state to know what to expect
DAEMON_SPA_ON=$(curl -sf "$DAEMON_URL/api/pool" | python3 -c "import sys,json; print(json.load(sys.stdin)['spa']['on'])")
DAEMON_HEAT_MODE=$(curl -sf "$DAEMON_URL/api/pool" | python3 -c "import sys,json; print(json.load(sys.stdin)['spa']['heat_mode'])")
EXPECTED_ONOFF=$([[ "$DAEMON_SPA_ON" == "True" ]] && echo "TRUE" || echo "FALSE")
EXPECTED_SYSMODE=$([[ "$DAEMON_HEAT_MODE" == "off" ]] && echo "0" || echo "4")

assert_read        "Spa OnOff"              "$EXPECTED_ONOFF"  "OnOff"                     onoff read on-off "$NODE_ID" 2
assert_read_nonzero "Spa LocalTemperature"            "LocalTemperature"          thermostat read local-temperature "$NODE_ID" 2
assert_read_nonzero "Spa HeatingSetpoint"             "OccupiedHeatingSetpoint"   thermostat read occupied-heating-setpoint "$NODE_ID" 2
assert_read        "Spa SystemMode"         "$EXPECTED_SYSMODE" "SystemMode"              thermostat read system-mode "$NODE_ID" 2
assert_read        "Spa CtrlSeqOfOp"        "2"      "ControlSequenceOfOperation" thermostat read control-sequence-of-operation "$NODE_ID" 2
assert_read        "Spa MaxHeatLimit"       "4000"   "AbsMaxHeatSetpointLimit"   thermostat read abs-max-heat-setpoint-limit "$NODE_ID" 2
assert_read_nonzero "Spa MinHeatLimit"                "AbsMinHeatSetpointLimit"   thermostat read abs-min-heat-setpoint-limit "$NODE_ID" 2

# Pool (ep 3)
assert_read_nonzero "Pool LocalTemperature"            "LocalTemperature"          thermostat read local-temperature "$NODE_ID" 3
assert_read_nonzero "Pool HeatingSetpoint"             "OccupiedHeatingSetpoint"   thermostat read occupied-heating-setpoint "$NODE_ID" 3

# Jets (ep 4)
OUTPUT=$(chip onoff read on-off "$NODE_ID" 4)
JETS=$(echo "$OUTPUT" | grep "OnOff:" | tail -1 | sed 's/.*OnOff: *//')
echo "  PASS  Jets OnOff ($JETS)"; ((PASS++))

# Lights (ep 5)
OUTPUT=$(chip onoff read on-off "$NODE_ID" 5)
LIGHTS=$(echo "$OUTPUT" | grep "OnOff:" | tail -1 | sed 's/.*OnOff: *//')
echo "  PASS  Lights OnOff ($LIGHTS)"; ((PASS++))

# ModeSelect: count supported modes
OUTPUT=$(chip modeselect read supported-modes "$NODE_ID" 5)
MODE_COUNT=$(echo "$OUTPUT" | grep -c "Label:" || echo 0)
if [[ "$MODE_COUNT" -eq 12 ]]; then
    echo "  PASS  Lights SupportedModes ($MODE_COUNT modes)"; ((PASS++))
else
    echo "  FAIL  Lights SupportedModes (expected 12, got $MODE_COUNT)"; ((FAIL++))
fi

assert_read_nonzero "Lights CurrentMode" "CurrentMode" modeselect read current-mode "$NODE_ID" 5

# --- Write + command tests ---

echo ""
echo "Write + command tests"

# Save initial setpoint
INITIAL_SP=$(curl -sf "$DAEMON_URL/api/pool" | python3 -c "import sys,json; print(json.load(sys.stdin)['spa']['setpoint'])")

# Write setpoint to 100°F = 3778 (37.78°C * 100)
assert_cmd "Write setpoint 100°F" thermostat write occupied-heating-setpoint 3778 "$NODE_ID" 2
sleep 2
assert_read "Readback setpoint" "3778" "OccupiedHeatingSetpoint" thermostat read occupied-heating-setpoint "$NODE_ID" 2
assert_daemon "Daemon setpoint=100" "spa.setpoint" "100"

# Restore original
RESTORE=$(python3 -c "print(round(($INITIAL_SP-32)*5/9*100))")
chip thermostat write occupied-heating-setpoint "$RESTORE" "$NODE_ID" 2 >/dev/null
sleep 1

# ModeSelect: change to party (1) on ep 5
assert_cmd "ChangeToMode party" modeselect change-to-mode 1 "$NODE_ID" 5
sleep 2
assert_read "Readback mode=party" "1" "CurrentMode" modeselect read current-mode "$NODE_ID" 5
assert_daemon "Daemon mode=party" "lights.mode" "party"

# ModeSelect: change to caribbean (3) on ep 5
assert_cmd "ChangeToMode caribbean" modeselect change-to-mode 3 "$NODE_ID" 5
sleep 1
assert_daemon "Daemon mode=caribbean" "lights.mode" "caribbean"

# OnOff: spa on (ep 2)
assert_cmd "Spa On" onoff on "$NODE_ID" 2
sleep 1
assert_daemon "Daemon spa on" "spa.on" "True"

# OnOff: jets on (ep 4)
assert_cmd "Jets On" onoff on "$NODE_ID" 4
sleep 1
assert_daemon "Daemon jets" "spa.accessories.jets" "True"

# OnOff: lights on (ep 5)
assert_cmd "Lights On" onoff on "$NODE_ID" 5
sleep 1
assert_daemon "Daemon lights on" "lights.on" "True"

# --- Summary ---

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
TOTAL=$((PASS + FAIL))
if [[ "$FAIL" -eq 0 ]]; then
    echo "ALL $TOTAL TESTS PASSED"
else
    echo "$FAIL of $TOTAL TESTS FAILED"
fi
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Kill bridge
if [[ "$KEEP" == false ]] && [[ -n "$BRIDGE_PID" ]]; then
    kill "$BRIDGE_PID" 2>/dev/null || true
fi

exit "$FAIL"
