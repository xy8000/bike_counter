#!/bin/sh
# Renders config.toml from environment variables, then starts the application.
set -e

if [ -z "${DATABASE_URL}" ] || [ -z "${DATABASE_USER}" ] || [ -z "${DATABASE_PASSWORD}" ] || [ -z "${DATABASE_NAME}" ]; then
  echo "ERROR: DATABASE_URL, DATABASE_USER, DATABASE_PASSWORD and DATABASE_NAME must be set" >&2
  exit 1
fi

cat > config.toml <<EOF
github_data_url="${GITHUB_DATA_URL:-https://github.com/od-ms/radverkehr-zaehlstellen/archive/refs/heads/main.zip}"
database_url="${DATABASE_URL}"
database_user="${DATABASE_USER}"
database_password="${DATABASE_PASSWORD}"
database_name="${DATABASE_NAME}"
EOF

exec /usr/local/bin/bike_counter
