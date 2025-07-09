#!/bin/sh
set -e

# Optionally start Shadowsocks client
if [ "$SS_CLIENT_ENABLE" = "1" ] || [ "$SS_CLIENT_ENABLE" = "true" ]; then
  echo ">>> [entrypoint] Starting Shadowsocks client..."
  sslocal &
fi

# Execute given command
exec "$@"
