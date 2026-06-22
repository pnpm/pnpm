---
"@pnpm/pnpr.client": patch
"pnpm": patch
---

The pnpr resolver endpoints moved under the reserved `/-/pnpr` namespace: `POST /v1/resolve` is now `POST /-/pnpr/v0/resolve` and `POST /v1/verify-lockfile` is now `POST /-/pnpr/v0/verify-lockfile`. The capability handshake at `GET /-/pnpr` advertises protocol version `0` to match. This keeps every pnpr-proprietary route in npm's reserved namespace, so it can never collide with a package path.
