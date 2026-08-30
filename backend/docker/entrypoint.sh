#!/bin/sh
set -e

if [ ! -f config.toml ]; then
  echo "ERROR: config.toml not found. Mount one into /app, e.g." >&2
  echo "  -v ./config.toml:/app/config.toml:ro" >&2
  exit 1
fi

# Forward arguments so the standalone `bike_counter tiles` subcommand (used by
# `make tiles` / `make tiles-update`) works through the backend image.
exec /usr/local/bin/bike_counter "$@"
