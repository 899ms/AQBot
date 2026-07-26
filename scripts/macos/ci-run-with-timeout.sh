# Sourced by CI signing scripts. macOS ships no native timeout(1), and brew
# coreutils' gtimeout is not guaranteed on PATH, so this is a pure-bash
# watchdog. Needed because `security` trust-settings commands can raise an
# interactive authorization dialog on hosted runners and block forever
# (see actions/runner-images#12116).
run_with_timeout() {
  local timeout_secs="$1"
  shift

  "$@" &
  local cmd_pid=$!

  (
    sleep "${timeout_secs}"
    kill -TERM "${cmd_pid}" 2>/dev/null
    sleep 5
    kill -KILL "${cmd_pid}" 2>/dev/null
  ) >/dev/null 2>&1 &
  local watchdog_pid=$!
  # keep bash from printing a "Terminated" job notice when we kill it below
  disown "${watchdog_pid}" 2>/dev/null || true

  local rc=0
  wait "${cmd_pid}" 2>/dev/null || rc=$?
  kill "${watchdog_pid}" 2>/dev/null || true
  return "${rc}"
}
