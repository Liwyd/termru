#!/usr/bin/env bash
# End-to-end smoke test of the deployment.
#
# The script starts the compose stack with the image under test and then
# checks the things a real deployment depends on:
#
#   1. the server becomes ready (the migration ran, the database is usable),
#   2. the schema of the migration is present,
#   3. a client can register, push a record and pull it back,
#   4. the database can be backed up and the archive restored into a second
#      database with the same content.
#
# Usage:
#   TERMRU_CLOUD_IMAGE=termru-cloud:ci \
#     ./apps/cloud-server/scripts/compose-smoke.sh
#
# Requirements: docker (with the compose plugin), curl, psql, pg_dump,
# pg_restore and python3. The test is meant for a Linux CI runner; it
# publishes the database on the loopback interface for the backup step
# (the deployment itself keeps the database private).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_DIR="$(dirname "$SCRIPT_DIR")"

IMAGE="${TERMRU_CLOUD_IMAGE:?TERMRU_CLOUD_IMAGE must name the image under test}"
PROJECT="${TERMRU_COMPOSE_PROJECT:-termru-smoke}"
DB_PORT="${TERMRU_SMOKE_DB_PORT:-5433}"
APP_URL="http://127.0.0.1:8787"

for tool in docker curl psql pg_dump pg_restore python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "compose-smoke: '$tool' is not installed" >&2
    exit 1
  fi
done

if ! docker compose version >/dev/null 2>&1; then
  echo "compose-smoke: the docker compose plugin is not available" >&2
  exit 1
fi

WORK="$(mktemp -d)"
COMPOSE=(docker compose -p "$PROJECT"
  -f "$COMPOSE_DIR/compose.yaml"
  -f "$WORK/compose.smoke.yaml"
  --env-file "$WORK/.env")

cleanup() {
  "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# --- the stack ---------------------------------------------------------------

PASSWORD="$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')"
DB_URL="postgres://termru:${PASSWORD}@127.0.0.1:${DB_PORT}/termru"
RESTORE_DB_URL="postgres://termru:${PASSWORD}@127.0.0.1:${DB_PORT}/termru_restored"

cat > "$WORK/.env" <<EOF
POSTGRES_PASSWORD=$PASSWORD
POSTGRES_USER=termru
POSTGRES_DB=termru
EOF

# The database is published for the backup step only; the deployment file
# itself keeps it private.
cat > "$WORK/compose.smoke.yaml" <<EOF
services:
  db:
    ports:
      - "127.0.0.1:${DB_PORT}:5432"
  app:
    image: ${IMAGE}
EOF

echo "compose-smoke: starting the stack with $IMAGE"
"${COMPOSE[@]}" up -d

fail() {
  echo "compose-smoke: FAILED — $1" >&2
  echo "--- the logs of the stack ---" >&2
  "${COMPOSE[@]}" logs --no-color >&2 || true
  exit 1
}

# --- 1. the server becomes ready ---------------------------------------------

ready=0
for _ in $(seq 1 120); do
  if curl -fsS "$APP_URL/readyz" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
[ "$ready" = 1 ] || fail "the server did not become ready within 120 s"
echo "compose-smoke: the server is ready"

curl -fsS "$APP_URL/healthz" >/dev/null || fail "the health check does not answer"

# --- 2. the migration ran ----------------------------------------------------

TABLES="$(psql "$DB_URL" -Atc \
  "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' ORDER BY table_name")" \
  || fail "the database is not reachable"

for table in users devices sessions refresh_tokens records applied_mutations audit_log login_attempts; do
  grep -qx "$table" <<<"$TABLES" || fail "the table '$table' is missing (the migration did not run)"
done
echo "compose-smoke: the schema of the migration is present"

# --- 3. a client round trip --------------------------------------------------

uuid() {
  if command -v uuidgen >/dev/null 2>&1; then
    uuidgen | tr '[:upper:]' '[:lower:]'
  else
    cat /proc/sys/kernel/random/uuid
  fi
}

DEVICE_ID="$(uuid)"
MUTATION_ID="$(uuid)"
RECORD_ID="$(uuid)"

json_field() {
  python3 -c 'import json,sys
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(1)
value = data
for key in sys.argv[1].split("."):
    if not isinstance(value, dict) or key not in value:
        sys.exit(1)
    value = value[key]
print(value)' "$1"
}

REGISTER="$(curl -fsS -X POST "$APP_URL/v1/auth/register" \
  -H 'content-type: application/json' \
  -d "{\"email\":\"smoke@example.test\",\"password\":\"smoke-test-password-1\",\"device_id\":\"${DEVICE_ID}\",\"device_name\":\"smoke-test\"}")" \
  || fail "the registration failed"

TOKEN="$(printf '%s' "$REGISTER" | json_field access_token || true)"
[ -n "$TOKEN" ] || fail "the registration did not return an access token"

DEVICES="$(curl -fsS "$APP_URL/v1/devices" -H "authorization: Bearer $TOKEN")" \
  || fail "the device list failed"
printf '%s' "$DEVICES" | python3 -c 'import json,sys
devices = json.load(sys.stdin)
assert len(devices) == 1, f"expected one device, got {len(devices)}"' \
  || fail "the registered device is not in the device list"

curl -fsS -X POST "$APP_URL/v1/sync/changes" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d "{\"mutations\":[{\"mutation_id\":\"${MUTATION_ID}\",\"id\":\"${RECORD_ID}\",\"kind\":\"host\",\"deleted\":false,\"payload\":{\"name\":\"smoke-host\",\"address\":\"smoke.example.test\"}}]}" \
  >/dev/null || fail "the push failed"

CHANGES="$(curl -fsS "$APP_URL/v1/sync/changes?since=0" -H "authorization: Bearer $TOKEN")" \
  || fail "the pull failed"
printf '%s' "$CHANGES" | python3 -c 'import json,sys
data = json.load(sys.stdin)
names = [r["payload"].get("name") for r in data["records"]]
assert "smoke-host" in names, f"the pushed record is missing from the pull: {names}"' \
  || fail "the pushed record did not come back"
echo "compose-smoke: register, push and pull work"

# --- 4. backup and restore ---------------------------------------------------

mkdir -p "$WORK/backup"
DUMP="$(TERMRU_DATABASE_URL="$DB_URL" "$COMPOSE_DIR/scripts/postgres-backup.sh" "$WORK/backup")" \
  || fail "the backup failed"
[ -f "$DUMP" ] || fail "the backup did not produce an archive"
[ -f "$DUMP.sha256" ] || fail "the backup did not produce a checksum"

sha256sum --check --status "$DUMP.sha256" || fail "the checksum of the backup does not match"

psql "$DB_URL" -c 'CREATE DATABASE termru_restored' >/dev/null \
  || fail "the restore target could not be created"
TERMRU_DATABASE_URL="$RESTORE_DB_URL" "$COMPOSE_DIR/scripts/postgres-restore.sh" "$DUMP" >/dev/null \
  || fail "the restore failed"

for table in users records; do
  SOURCE="$(psql "$DB_URL" -Atc "SELECT count(*) FROM $table")"
  RESTORED="$(psql "$RESTORE_DB_URL" -Atc "SELECT count(*) FROM $table")"
  [ "$SOURCE" = "$RESTORED" ] \
    || fail "the restored table '$table' has $RESTORED rows, the source has $SOURCE"
done
[ "$(psql "$RESTORE_DB_URL" -Atc 'SELECT count(*) FROM records')" = "1" ] \
  || fail "the restored database does not contain the pushed record"
echo "compose-smoke: the backup and the restore work"

echo "compose-smoke: OK"
