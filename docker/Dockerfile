# syntax=docker/dockerfile:1.7

FROM debian:stable-slim

ARG PNPM_VERSION
ARG TARGETARCH

ENV PNPM_HOME=/pnpm
ENV PATH=$PNPM_HOME:$PATH

RUN set -eu; \
    test -n "$PNPM_VERSION"; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl; \
    rm -rf /var/lib/apt/lists/*; \
    case "$TARGETARCH" in \
      amd64) arch=x64 ;; \
      arm64) arch=arm64 ;; \
      *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    mkdir -p /opt/pnpm "$PNPM_HOME"; \
    curl -fsSL "https://github.com/pnpm/pnpm/releases/download/v${PNPM_VERSION}/pnpm-linux-${arch}.tar.gz" \
      | tar -xz -C /opt/pnpm; \
    ln -s /opt/pnpm/pnpm /usr/local/bin/pnpm; \
    installed="$(pnpm --version)"; \
    test "$installed" = "$PNPM_VERSION" || { \
      echo "pnpm version mismatch: expected $PNPM_VERSION, got $installed" >&2; \
      exit 1; \
    }

WORKDIR /app
