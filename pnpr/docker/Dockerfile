# syntax=docker/dockerfile:1.7

# Refresh periodically: resolve with
#   docker buildx imagetools inspect debian:stable-slim --format '{{.Manifest.Digest}}'
FROM debian:stable-slim@sha256:e51bfcd2226c480a5416730e0fa2c40df28b0da5ff562fc465202feeef2f1116

ARG PNPR_VERSION
ARG TARGETARCH
ARG PNPR_SHA256_AMD64
ARG PNPR_SHA256_ARM64

# pnpr reaches upstream registries over HTTPS via rustls' platform
# verifier, which reads the system CA bundle, so ca-certificates is required.
RUN set -eu; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates; \
    rm -rf /var/lib/apt/lists/*; \
    useradd --system --create-home --home-dir /home/pnpr --shell /usr/sbin/nologin pnpr

# The build context stages the prebuilt static musl binary for each
# architecture as pnpr-<TARGETARCH> (pnpr-amd64, pnpr-arm64).
COPY pnpr-${TARGETARCH} /usr/local/bin/pnpr

# Verify the staged binary against the checksum pinned by the release job
# before trusting it. `pnpr --version` only confirms reported metadata, so
# it cannot stand in for an integrity check on the binary itself.
RUN set -eu; \
    test -n "$PNPR_VERSION"; \
    case "$TARGETARCH" in \
      amd64) expected_sha="$PNPR_SHA256_AMD64" ;; \
      arm64) expected_sha="$PNPR_SHA256_ARM64" ;; \
      *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    test -n "$expected_sha" || { echo "missing PNPR_SHA256_* build-arg for $TARGETARCH" >&2; exit 1; }; \
    echo "$expected_sha  /usr/local/bin/pnpr" | sha256sum -c -; \
    chmod 0755 /usr/local/bin/pnpr; \
    installed="$(pnpr --version | awk '{print $NF}')"; \
    test "$installed" = "$PNPR_VERSION" || { \
      echo "pnpr version mismatch: expected $PNPR_VERSION, got $installed" >&2; \
      exit 1; \
    }; \
    mkdir -p /pnpr/storage /pnpr/cache; \
    chown -R pnpr:pnpr /pnpr

USER pnpr
WORKDIR /pnpr

# Persist published packages and the disposable upstream mirror.
VOLUME ["/pnpr/storage", "/pnpr/cache"]

# pnpr's default port. Bind to 0.0.0.0 so the server is reachable from
# outside the container (the binary defaults to 127.0.0.1).
EXPOSE 7677

ENTRYPOINT ["pnpr"]
CMD ["--listen", "0.0.0.0:7677", "--storage", "/pnpr/storage", "--cache", "/pnpr/cache"]
