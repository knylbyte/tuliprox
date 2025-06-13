#!/bin/sh
set -e # Exit immediately if a command exits with a non-zero status.

# --- LLDB Server Configuration ---
# Read port configuration from environment variables, with sane defaults.
LLDB_SERVER_PORT=${LLDB_SERVER_PORT:-10586}
LLDB_MIN_PORT=${LLDB_MIN_PORT:-10600}
LLDB_MAX_PORT=${LLDB_MAX_PORT:-10700}


# --- Build Step ---
# 1. Ensure the Rust binary is up-to-date.
echo ">>> [debug-entrypoint] Ensuring the binary is up-to-date for target: ${RUST_TARGET}..."
cargo build --target "${RUST_TARGET}"


# --- LLDB Server Start ---
# 2. Start lldb-server in platform mode in the background.
#    It listens on the main control port and uses a dedicated range for communication.
echo ">>> [debug-entrypoint] Starting lldb-server..."
echo "    Control Port: ${LLDB_SERVER_PORT}"
echo "    Session Ports: ${LLDB_MIN_PORT}-${LLDB_MAX_PORT}"

/usr/bin/lldb-server platform \
  --listen "*:${LLDB_SERVER_PORT}" \
  --server \
  --min-gdbserver-port ${LLDB_MIN_PORT} \
  --max-gdbserver-port ${LLDB_MAX_PORT} &

# --- Application Start ---
# 3. Start the actual application in the foreground.
#    `exec` replaces the shell process, making the application the main process.
echo ">>> [debug-entrypoint] Starting application 'tuliprox'. Ready for debugger to attach..."
exec /usr/src/tuliprox/target/${RUST_TARGET}/debug/tuliprox -s -p /app/config