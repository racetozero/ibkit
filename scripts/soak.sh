#!/usr/bin/env bash
# Long-running resilience test of ibkit against a real (paper) account.
#
# It starts the controller, waits until Gateway is ready, and then, in turn,
# injects one fault per interval and measures the time until Gateway is
# ready again:
#   controller  kill -9 the controller and start a new one (it must adopt
#               the running Gateway without a new login)
#   jvm         kill -9 the Gateway JVM (automatic recovery logs in again)
#   restart     `ibkit gateway restart` (a clean new login)
#   reconnect   `ibkit gateway reconnect-account`
# Between faults it checks readiness every 30 seconds and records each
# unexpected outage. The report is one JSON object per line; the last line
# is the summary. The exit status is 1 when a fault did not recover in time
# or an outage did not end within the ready timeout.
#
# The controller reads its settings as usual: IBKIT_CONFIG or ./config.toml,
# and IBKIT_* variables (for example IBKIT_TRADING_MODE=paper, IB_USERNAME,
# IB_PASSWORD). Use a dedicated paper account: the faults end its sessions.
#
# Usage: scripts/soak.sh [--hours 48] [--fault-every-mins 180]
#                        [--ready-timeout-mins 45] [--report soak.jsonl]
#                        [--log soak-controller.log]
# IBKIT_BINARY selects the binary (default target/release/ibkit).
set -euo pipefail

hours=48
fault_every=180
ready_timeout=45
report=soak.jsonl
log=soak-controller.log
while [ $# -gt 0 ]; do
  case "$1" in
    --hours) hours=$2; shift 2 ;;
    --fault-every-mins) fault_every=$2; shift 2 ;;
    --ready-timeout-mins) ready_timeout=$2; shift 2 ;;
    --report) report=$2; shift 2 ;;
    --log) log=$2; shift 2 ;;
    *) echo "You gave the unknown option $1. Use --hours, --fault-every-mins, --ready-timeout-mins, --report or --log." >&2; exit 2 ;;
  esac
done
for value in "$hours" "$fault_every" "$ready_timeout"; do
  case "$value" in ''|*[!0-9]*|0) echo "You gave $value as a duration. Use a whole number above 0." >&2; exit 2 ;; esac
done
ibkit=${IBKIT_BINARY:-target/release/ibkit}
if [ ! -x "$ibkit" ]; then
  echo "$ibkit is not an executable. Build it with \`cargo build --release -p ibkit-cli\` or set IBKIT_BINARY." >&2
  exit 2
fi
command -v jq >/dev/null || { echo 'jq is not installed. Install jq; the script reads controller status with it.' >&2; exit 2; }
"$ibkit" gateway validate --credentials

controller=
start_controller() {
  "$ibkit" gateway run >>"$log" 2>&1 &
  controller=$!
}
stop_controller() {
  if [ -n "$controller" ] && kill -0 "$controller" 2>/dev/null; then
    "$ibkit" gateway stop || kill "$controller"
    wait "$controller" || true
  fi
}
trap stop_controller EXIT

record() { printf '%s\n' "$1" >>"$report"; printf '%s\n' "$1"; }
now() { date +%s; }

# Seconds until Gateway is ready, or -1 after the ready timeout.
await_ready() {
  local started deadline
  started=$(now)
  deadline=$((started + ready_timeout * 60))
  while [ "$(now)" -lt "$deadline" ]; do
    if "$ibkit" gateway health --kind ready >/dev/null 2>&1; then
      echo $(($(now) - started))
      return
    fi
    sleep 5
  done
  echo -1
}

: >"$report"
start_controller
first=$(await_ready)
record "{\"at\":$(now),\"step\":\"initial_login\",\"seconds\":$first}"
if [ "$first" -lt 0 ]; then
  echo "Gateway did not become ready in $ready_timeout minutes. Read $log and check the credentials and the second factor." >&2
  exit 1
fi

faults=(controller jvm restart reconnect)
deadline=$(($(now) + hours * 3600))
count=0
failures=0
outages=0
slowest=0
while [ "$(now)" -lt "$deadline" ]; do
  next=$(($(now) + fault_every * 60))
  while [ "$(now)" -lt "$next" ] && [ "$(now)" -lt "$deadline" ]; do
    sleep 30
    if ! "$ibkit" gateway health --kind ready >/dev/null 2>&1; then
      outages=$((outages + 1))
      seconds=$(await_ready)
      record "{\"at\":$(now),\"step\":\"outage\",\"seconds\":$seconds}"
      if [ "$seconds" -lt 0 ]; then failures=$((failures + 1)); fi
    fi
  done
  [ "$(now)" -lt "$deadline" ] || break
  fault=${faults[$((count % ${#faults[@]}))]}
  count=$((count + 1))
  case "$fault" in
    controller)
      kill -9 "$controller"
      wait "$controller" 2>/dev/null || true
      start_controller
      ;;
    jvm)
      for pid in $("$ibkit" gateway status | jq -r '.agents[].pid'); do kill -9 "$pid" 2>/dev/null || true; done
      ;;
    restart) "$ibkit" gateway restart ;;
    reconnect) "$ibkit" gateway reconnect-account ;;
  esac
  # Let the fault take effect before readiness counts again.
  sleep 10
  seconds=$(await_ready)
  if [ "$seconds" -lt 0 ]; then failures=$((failures + 1)); elif [ "$seconds" -gt "$slowest" ]; then slowest=$seconds; fi
  record "{\"at\":$(now),\"step\":\"fault\",\"fault\":\"$fault\",\"seconds\":$seconds}"
done

record "{\"at\":$(now),\"step\":\"summary\",\"hours\":$hours,\"faults\":$count,\"outages\":$outages,\"failures\":$failures,\"slowest_recovery_seconds\":$slowest}"
[ "$failures" -eq 0 ]
