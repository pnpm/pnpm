---
"@pnpm/pnpr": patch
---

pnpr now answers the first request after its remote libSQL database sat idle for a few seconds or restarted. Such a request used to fail with HTTP 500, and `/-/ping` and `/-/ready` with 503, on a `STREAM_EXPIRED` or "invalid baton" error from `sqld` or Turso.
