#!/usr/bin/env bash
# Back up the Termru cloud database.
#
# Usage:
#   TERMRU_DATABASE_URL=postgres://user:pass@host:5432/termru \
#     ./apps/cloud-server/scripts/postgres-backup.sh [output-directory]
#
# The dump is a custom format archive (compressed, restorable with
# pg_restore) plus a SHA-256 checksum file.
set -euo pipefail

: "${TERMRU_DATABASE_URL:?TERMRU_DATABASE_URL must be set}"
OUTPUT_DIR="${1:-./backups}"

if ! command -v pg_dump >/dev/null 2>&1; then
  echo "pg_dump is not installed" >&2
  exit 1
fi

mkdir -p "$OUTPUT_DIR"
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
DUMP_FILE="$OUTPUT_DIR/termru-$TIMESTAMP.dump"

# --no-owner keeps the archive restorable by a different role.
pg_dump --dbname="$TERMRU_DATABASE_URL" --format=custom --no-owner --file="$DUMP_FILE"

sha256sum "$DUMP_FILE" > "$DUMP_FILE.sha256"
chmod 600 "$DUMP_FILE" "$DUMP_FILE.sha256"

echo "$DUMP_FILE"
