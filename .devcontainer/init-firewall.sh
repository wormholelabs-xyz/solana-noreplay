#!/bin/bash
# Starts the castellan DNS-driven egress firewall. Run as root (via sudo).
#
# The heavy lifting lives in the `castellan` Rust binary, which is now fully
# self-bootstrapping: a single supervised `castellan daemon` binds its resolver socket,
# installs the default-drop + DNS-intercept nftables ruleset atomically, repoints
# resolv.conf, and then serves. This script only installs the binary and (re)launches the
# supervisor — no upstream capture, no multi-phase setup/enable-intercept dance.
#
# Idempotent: safe to run on both postCreate and postStart. On a container restart the
# network namespace (and thus the nftables ruleset) is fresh, so the daemon rebuilds it.
set -euo pipefail
IFS=$'\n\t'

BINARY=/usr/local/bin/castellan
SUPERVISOR=/usr/local/bin/castellan-supervisor.sh
READY=/run/castellan/ready
LOG=/var/log/castellan.log
PORT=53

if [ "$(id -u)" -ne 0 ]; then
  echo "ERROR: must run as root (use sudo)" >&2
  exit 1
fi

if [ ! -x "$BINARY" ]; then
  echo "ERROR: $BINARY not found" >&2
  exit 1
fi

# Stop any previous instance, then (re)start the supervised daemon. It self-bootstraps the
# firewall on startup. pkill only *sends* signals and returns immediately, so we must wait
# for the old processes to actually exit before launching a new one — otherwise the new
# daemon races the old one to bind port 53 and loses (missing the readiness window below).
rm -f "$READY"

# Wait (up to ~5s) for processes matching a pattern to exit, escalating to SIGKILL.
wait_gone() {
  local pattern="$1" i
  for i in $(seq 1 25); do
    pgrep -f "$pattern" >/dev/null 2>&1 || return 0
    sleep 0.2
  done
  pkill -KILL -f "$pattern" 2>/dev/null || true
  for i in $(seq 1 25); do
    pgrep -f "$pattern" >/dev/null 2>&1 || return 0
    sleep 0.2
  done
  return 1
}

# Kill the supervisor first so it stops respawning the daemon, then the daemon itself.
pkill -f "$SUPERVISOR" 2>/dev/null || true
wait_gone "$SUPERVISOR" || { echo "ERROR: previous supervisor would not exit" >&2; exit 1; }
pkill -f "$BINARY daemon" 2>/dev/null || true
wait_gone "$BINARY daemon" || { echo "ERROR: previous daemon would not exit" >&2; exit 1; }

mkdir -p "$(dirname "$READY")"
PORT="$PORT" setsid "$SUPERVISOR" >/dev/null 2>&1 </dev/null &

# Wait (up to ~10s) for the daemon to finish bootstrapping and advertise readiness.
for _ in $(seq 1 50); do
  [ -f "$READY" ] && break
  sleep 0.2
done
if [ ! -f "$READY" ]; then
  echo "ERROR: daemon did not become ready in time. Recent log:" >&2
  tail -n 20 "$LOG" 2>/dev/null || true
  exit 1
fi
echo "Daemon is ready."

# Verify end-to-end.
"$BINARY" verify

echo "castellan firewall is active."
