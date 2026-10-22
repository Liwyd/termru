# syntax=docker/dockerfile:1.7
#
# Termru cloud server — multi-stage build.
#
# The runtime image contains the server binary and nothing else: no source,
# no build toolchain, no package manager state. It runs as a non-root user
# and uses dumb-init as PID 1, so that SIGTERM reaches the server and the
# graceful shutdown actually happens.

# --- build ------------------------------------------------------------------
FROM rust:1.99-bookworm AS builder

WORKDIR /build

# The manifests are copied first so that the (slow) dependency build is
# cached independently of the source.
COPY Cargo.toml Cargo.lock ./
COPY crates/termru-core/Cargo.toml crates/termru-core/
COPY apps/cloud-server/Cargo.toml apps/cloud-server/

# Reduce the workspace to the crates the server needs: the Tauri desktop app
# would drag the whole webview stack into the image build.
RUN sed -i \
      -e '/crates\/termru-store/d' \
      -e '/crates\/termru-ssh/d' \
      -e '/apps\/desktop\/src-tauri/d' \
      Cargo.toml \
 && mkdir -p crates/termru-core/src apps/cloud-server/src apps/cloud-server/migrations \
 && echo "" > crates/termru-core/src/lib.rs \
 && echo "" > apps/cloud-server/src/lib.rs \
 && echo 'fn main() {}' > apps/cloud-server/src/main.rs \
 && cargo build --release -p termru-cloud-server --bin termru-cloud-server

# The real sources. The migrations are embedded into the binary by the build
# script, so they have to be present before the final build.
COPY crates/termru-core/src crates/termru-core/src
COPY apps/cloud-server/src apps/cloud-server/src
COPY apps/cloud-server/migrations apps/cloud-server/migrations
# The build script embeds the migrations into the binary: without it the
# `include!` of the generated table has no OUT_DIR to read.
COPY apps/cloud-server/build.rs apps/cloud-server/

RUN touch crates/termru-core/src/lib.rs apps/cloud-server/src/lib.rs \
 && cargo build --release -p termru-cloud-server --bin termru-cloud-server

# --- runtime ----------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# ca-certificates: TLS roots for a remote database.
# curl: used by the health check.
# dumb-init: PID 1 that forwards signals.
RUN apt-get update \
 && apt-get install --no-install-recommends -y \
      ca-certificates \
      curl \
      dumb-init \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --system --gid 10001 termru \
 && useradd --system --uid 10001 --gid 10001 --home-dir /nonexistent --shell /usr/sbin/nologin termru

COPY --from=builder /build/target/release/termru-cloud-server /usr/local/bin/termru-cloud-server

USER termru
WORKDIR /home/termru

ENV TERMRU_BIND_ADDR=0.0.0.0:8787 \
    RUST_LOG=info

EXPOSE 8787

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8787/readyz || exit 1

ENTRYPOINT ["/usr/bin/dumb-init", "--"]
CMD ["termru-cloud-server"]
