#!/usr/bin/env bash
# Restore a Termru cloud database from a backup.
#
# Usage:
#   TERMRU_DATABASE_URL=postgres://user:pass@host:5432/termru \
#     ./apps/cloud-server/scripts/postgres-restore.sh <dump-file>
#
# The current content of the target database is replaced. The script refuses
# to run when the checksum of the archive does not match.
set -euo pipefail

: "${TERMRU_DATABASE_URL:?TERMRU_DATABASE_URL must be set}"
DUMP_FILE="${1:?usage: postgres-restore.sh <dump-file>}"

if ! command -v pg_restore >/dev/null 2>&1; then
  echo "pg_restore is not installed" >&2
  exit 1
fi

if [ ! -f "$DUMP_FILE" ]; then
  echo "the archive $DUMP_FILE does not exist" >&2
  exit 1
fi

if [ -f "$DUMP_FILE.sha256" ]; then
  sha256sum --check --status "$DUMP_FILE.sha256"
  echo "checksum of $DUMP_FILE verified"
else
  echo "warning: no checksum file next to the archive, skipping the check" >&2
fi

# --clean removes the objects of the previous content; --if-exists keeps the
# restore running when an object is missing.
pg_restore --dbname="$TERMRU_DATABASE_URL" --clean --if-exists --no-owner --exit-on-error "$DUMP_FILE"

echo "restored $DUMP_FILE into $TERMRU_DATABASE_URL"
