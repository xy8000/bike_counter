#!/bin/sh
set -e

if [ ! -f config.toml ]; then
  echo "ERROR: config.toml not found. Mount one into /app, e.g." >&2
  echo "  -v ./config.toml:/app/config.toml:ro" >&2
  exit 1
fi

exec /usr/local/bin/bike_counter
