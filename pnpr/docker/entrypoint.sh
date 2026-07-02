#!/bin/sh
# Translate PNPR_* environment variables into pnpr CLI flags so the
# server can be configured entirely through a PaaS's env-var UI.
set -eu

set -- --listen "${PNPR_LISTEN:-0.0.0.0:4873}" --storage "${PNPR_STORAGE:-/data}"

if [ -n "${PNPR_CONFIG:-}" ]; then
  set -- "$@" --config "$PNPR_CONFIG"
fi

# Required behind a reverse proxy: pnpr rewrites dist.tarball URLs in
# served packuments to this URL. Without it, clients receive links
# pointing at the container's bind address.
if [ -n "${PNPR_PUBLIC_URL:-}" ]; then
  set -- "$@" --public-url "$PNPR_PUBLIC_URL"
fi

if [ -n "${PNPR_PACKUMENT_TTL_SECS:-}" ]; then
  set -- "$@" --packument-ttl-secs "$PNPR_PACKUMENT_TTL_SECS"
fi

exec pnpr "$@"
