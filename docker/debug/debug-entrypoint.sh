#!/bin/sh
set -e # Exit immediately if a command exits with a non-zero status.

# --- General Configuration ---
# Use DEBUG_SERVER environment variable to choose the debugger. Defaults to 'gdb'.
DEBUG_SERVER=${DEBUG_SERVER:-gdb}
# Default Rust target, can be overridden.
RUST_TARGET=${RUST_TARGET:-x86_64-unknown-linux-musl}
# Define paths and arguments once to keep it DRY.
BINARY_PATH="/usr/src/tuliprox/target/${RUST_TARGET}/debug/tuliprox"
APP_ARGS="-s -p /app/config"

# --- Debugger Ports ---
GDB_SERVER_PORT=${GDB_SERVER_PORT:-10586}
LLDB_SERVER_PORT=${LLDB_SERVER_PORT:-10586}
LLDB_MIN_PORT=${LLDB_MIN_PORT:-10600}
LLDB_MAX_PORT=${LLDB_MAX_PORT:-10700}


# --- Build Step (common for both) ---
echo ">>> [debug-entrypoint] Ensuring the binary is up-to-date for target: ${RUST_TARGET}..."
cargo build -p tuliprox --target "${RUST_TARGET}"


# --- Start Debug Server based on selection ---
case "${DEBUG_SERVER}" in
  gdb)
    # GDB: Launch the application *through* gdbserver.
    # The app will wait for a debugger to connect before starting.
    echo ">>> [debug-entrypoint] Starting GDB server..."
    echo "    Mode: Launching application via gdbserver"
    echo "    Listening on: 0.0.0.0:${GDB_SERVER_PORT}"
    exec /usr/bin/gdbserver "0.0.0.0:${GDB_SERVER_PORT}" ${BINARY_PATH} ${APP_ARGS}
    ;;

  lldb)
    # LLDB: Start lldb-server in the background, then start the application.
    # The debugger will attach to the already running application.
    echo ">>> [debug-entrypoint] Starting LLDB server..."
    echo "    Mode: Platform mode (attach to running process)"
    echo "    Control Port: ${LLDB_SERVER_PORT}"
    echo "    Session Ports: ${LLDB_MIN_PORT}-${LLDB_MAX_PORT}"

    /usr/bin/lldb-server platform \
      --listen "0.0.0.0:${LLDB_SERVER_PORT}" \
      --server \
      --min-gdbserver-port ${LLDB_MIN_PORT} \
      --max-gdbserver-port ${LLDB_MAX_PORT} &
    
    # Wait a moment for lldb-server to initialize before starting the app.
    sleep 1

    echo ">>> [debug-entrypoint] Starting application 'tuliprox'. Ready for debugger to attach..."
    exec ${BINARY_PATH} ${APP_ARGS}
    ;;

  *)
    # Error case for invalid DEBUG_SERVER value.
    echo "!!! [debug-entrypoint] ERROR: Invalid DEBUG_SERVER value '${DEBUG_SERVER}'." >&2
    echo "    Supported values are 'gdb' or 'lldb'." >&2
    exit 1
    ;;
esac