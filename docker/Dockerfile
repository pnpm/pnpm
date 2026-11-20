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
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl libatomic1; \
    rm -rf /var/lib/apt/lists/*

RUN --mount=type=bind,source=install-pnpm.sh,target=/tmp/install-pnpm.sh \
    PNPM_ASSET_SUFFIX= sh /tmp/install-pnpm.sh

WORKDIR /app
