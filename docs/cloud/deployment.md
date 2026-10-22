# Deploying the Termru cloud server

The cloud server is optional: the desktop app works entirely offline. Run it
only if you want your host configuration to follow you across devices.

The server is meant to run next to its database, on a machine you control.
It is **not** a multi-tenant SaaS: one deployment serves the accounts you
create in it.

## What you need

* A machine with Docker and the Compose plugin.
* A domain name and TLS termination (a reverse proxy in front of the server).
* About 100 MB of disk for the image and the database.

## Install

Fetch the versioned compose file, verify its checksum, and start it:

```bash
mkdir -p termru-cloud && cd termru-cloud
curl -fsSLO https://github.com/Liwyd/termru/releases/download/v0.1.0/compose.yaml
curl -fsSLO https://github.com/Liwyd/termru/releases/download/v0.1.0/compose.yaml.sha256
sha256sum --check compose.yaml.sha256
curl -fsSLO https://github.com/Liwyd/termru/releases/download/v0.1.0/termru-cloud.env.example
mv termru-cloud.env.example .env
```

The checksum check must succeed before the next step. Nothing is piped into a
shell: the file is downloaded, verified and then read by `docker compose`.

Now put a database password into `.env` and start the services:

```bash
# Generate a password and write it into .env
sed -i "s|^POSTGRES_PASSWORD=.*|POSTGRES_PASSWORD=$(openssl rand -base64 32)|" .env
chmod 600 .env

docker compose up -d
docker compose ps          # both services must become healthy
```

The migrations run automatically when the app container starts. The runner
holds a PostgreSQL advisory lock, so several replicas can start at the same
time without racing each other.

Check that the server answers:

```bash
curl -fsS http://127.0.0.1:8787/readyz
```

## Exposing it over TLS

`compose.yaml` binds the app to `127.0.0.1` on purpose: the server speaks
plain HTTP and expects a reverse proxy in front of it. A minimal Caddy
configuration:

```caddyfile
termru.example.com {
    reverse_proxy 127.0.0.1:8787
}
```

Caddy obtains and renews the certificate on its own. The desktop client then
uses `https://termru.example.com` as the server URL.

## First run

Create the first account — the server has no seeding and no default user:

```bash
curl -fsS -X POST https://termru.example.com/v1/auth/register \
  -H 'content-type: application/json' \
  -d '{"email":"you@example.com","password":"a long passphrase","device_id":"11111111-1111-4111-8111-111111111111","device_name":"my laptop"}'
```

The response contains the access and refresh tokens. The desktop client does
this for you; the command above is for the first setup and for scripts.

## Backups

The database holds the whole state of the service, so a backup is a
`pg_dump` of the database volume:

```bash
mkdir -p backups
docker compose exec -T db pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
  --format=custom --no-owner > "backups/termru-$(date -u +%Y%m%dT%H%M%SZ).dump"
sha256sum backups/termru-*.dump > backups/SHA256SUMS
```

Store the archive somewhere else than the machine that runs the service. To
restore:

```bash
sha256sum --check backups/SHA256SUMS
docker compose exec -T db psql -U "$POSTGRES_USER" -d postgres -c 'DROP DATABASE IF EXISTS termru WITH (FORCE)'
docker compose exec -T db psql -U "$POSTGRES_USER" -d postgres -c 'CREATE DATABASE termru'
docker compose exec -T db pg_restore -U "$POSTGRES_USER" -d termru --no-owner --exit-on-error \
  < backups/termru-<timestamp>.dump
```

The same scripts ship with the source in
`apps/cloud-server/scripts/postgres-backup.sh` and
`apps/cloud-server/scripts/postgres-restore.sh`.

## Upgrading and rolling back

```bash
# 1. back up (see above)
# 2. change the version in .env
sed -i 's|^TERMRU_CLOUD_VERSION=.*|TERMRU_CLOUD_VERSION=0.1.1|' .env
# 3. apply
docker compose pull
docker compose up -d
```

The migrations are forward-only. A rollback therefore means restoring the
backup taken before the upgrade, not running the old image against the new
schema. If a release contains a migration that must not be applied
automatically, stop the app service before the upgrade and run the migration
by hand:

```bash
docker compose stop app
docker compose run --rm app termru-cloud-server --help   # (placeholder: see the release notes)
```

## Rotating the database password

1. Change the password inside the database:
   `docker compose exec -T db psql -U "$POSTGRES_USER" -d postgres -c "ALTER ROLE termru WITH PASSWORD 'new-password'"`
2. Write the new password into `.env`.
3. Restart the app: `docker compose up -d app`

There is a short window in which the app still uses the old password; the
app reconnects on its own, so restarting it directly is enough.

## Rotating an account token

Access tokens are short-lived (one hour by default) and refresh tokens
rotate on every use. To invalidate every session of an account, revoke its
devices in the desktop app — a revoked device loses API access immediately.

## Troubleshooting

| Symptom | What to check |
|---|---|
| `app` restarts repeatedly | `docker compose logs app` — usually a wrong `TERMRU_DATABASE_URL` or a missing `POSTGRES_PASSWORD` |
| `readyz` answers `503` | the database is not reachable: `docker compose ps` and `docker compose logs db` |
| `429` from the API | the rate limiter: raise `TERMRU_RATE_LIMIT_PER_MINUTE` in `.env` |
| `401` right after signing in | the system clock of the server and of the client differ by more than the token lifetime |
