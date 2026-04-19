# syntax=docker/dockerfile:1.7

# Refresh periodically: resolve with
#   docker buildx imagetools inspect debian:stable-slim --format '{{.Manifest.Digest}}'
FROM debian:stable-slim@sha256:e51bfcd2226c480a5416730e0fa2c40df28b0da5ff562fc465202feeef2f1116

ARG PNPM_VERSION
ARG PNPM_SHA256_AMD64
ARG PNPM_SHA256_ARM64
ARG TARGETARCH

ENV PNPM_HOME=/pnpm
ENV PATH=$PNPM_HOME/bin:$PATH

RUN set -eu; \
    test -n "$PNPM_VERSION"; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl libatomic1; \
    rm -rf /var/lib/apt/lists/*; \
    case "$TARGETARCH" in \
      amd64) arch=x64; expected_sha="$PNPM_SHA256_AMD64" ;; \
      arm64) arch=arm64; expected_sha="$PNPM_SHA256_ARM64" ;; \
      *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    test -n "$expected_sha" || { echo "missing PNPM_SHA256_* build-arg for $TARGETARCH" >&2; exit 1; }; \
    mkdir -p /opt/pnpm "$PNPM_HOME/bin"; \
    chmod -R a+rwX "$PNPM_HOME"; \
    curl -fsSL --retry 3 --retry-delay 2 -o /tmp/pnpm.tgz \
      "https://github.com/pnpm/pnpm/releases/download/v${PNPM_VERSION}/pnpm-linux-${arch}.tar.gz"; \
    actual_sha="$(sha256sum /tmp/pnpm.tgz | awk '{print $1}')"; \
    test "$actual_sha" = "$expected_sha" || { \
      echo "sha256 mismatch for pnpm-linux-${arch}.tar.gz: expected $expected_sha, got $actual_sha" >&2; \
      exit 1; \
    }; \
    tar -xzf /tmp/pnpm.tgz -C /opt/pnpm; \
    rm /tmp/pnpm.tgz; \
    ln -s /opt/pnpm/pnpm /usr/local/bin/pnpm; \
    installed="$(pnpm --version)"; \
    test "$installed" = "$PNPM_VERSION" || { \
      echo "pnpm version mismatch: expected $PNPM_VERSION, got $installed" >&2; \
      exit 1; \
    }

WORKDIR /app
