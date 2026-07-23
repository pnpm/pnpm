# Build context must be the monorepo root, since pnpr depends on
# pacquet-* crates from the shared Cargo workspace:
#
#   docker build -f pnpr/Dockerfile -t pnpr .
#
# The companion pnpr/docker-compose.yml sets the context for you.

# The pinned toolchain matches rust-toolchain.toml at the repo root.
FROM rust:1.95.0-slim-bookworm AS builder

# cmake + perl are needed by aws-lc-sys/ring (pulled in via reqwest's
# rustls backend); build-essential provides the C compiler that
# rusqlite's bundled SQLite and the *-sys crates compile against.
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential \
      cmake \
      perl \
      pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .
RUN cargo build --release --locked --bin pnpr

FROM debian:bookworm-slim AS runtime

# ca-certificates lets pnpr's reqwest client validate TLS to upstream
# registries (registry.npmjs.org and any configured uplinks).
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
      curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/pnpr /usr/local/bin/pnpr
COPY pnpr/docker/config.yaml /etc/pnpr/config.yaml
COPY pnpr/docker/entrypoint.sh /usr/local/bin/pnpr-entrypoint
RUN chmod +x /usr/local/bin/pnpr-entrypoint

# Storage, auth state (htpasswd, tokens.db), and the packument/tarball
# cache all live under /data. Mount it as a volume so they survive
# redeploys.
ENV PNPR_STORAGE=/data \
    PNPR_LISTEN=0.0.0.0:4873 \
    PNPR_CONFIG=/etc/pnpr/config.yaml
VOLUME /data
EXPOSE 4873

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:4873/-/ping || exit 1

ENTRYPOINT ["/usr/local/bin/pnpr-entrypoint"]
