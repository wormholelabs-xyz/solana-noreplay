#!/bin/bash
# Supervises the castellan DNS resolver daemon, restarting it on crash.
#
# There is no init system in the devcontainer base image, so this loop is the fail-closed
# safety net: if the daemon dies, no new DNS resolution happens (existing connections
# survive via conntrack), and this restarts it within ~1s. Logs go to /var/log/castellan.log.
#
# Launched detached (setsid) by init-firewall.sh. Optionally honors PORT (default 53). The
# daemon self-detects its upstream resolver(s), so no UPSTREAM is needed here.
set -u

BINARY=/usr/local/bin/castellan
LOG=/var/log/castellan.log
PORT="${PORT:-53}"

echo "[$(date)] supervisor starting (port=${PORT})" >>"$LOG"
while true; do
  echo "[$(date)] launching castellan daemon" >>"$LOG"
  "$BINARY" daemon --listen "127.0.0.1:${PORT}" >>"$LOG" 2>&1
  rc=$?
  echo "[$(date)] daemon exited (rc=${rc}); restarting in 1s" >>"$LOG"
  sleep 1
done
