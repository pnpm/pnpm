---
"@pnpm/pnpr": patch
---

`POST /-/pnpr/v0/resolve` and `POST /-/pnpr/v0/verify-lockfile` now limit request bodies to 32 MiB. Concurrent and queued requests are capped per server instance and per caller identity. Streaming resolves abort when the client disconnects. Resolves enforce `requestTimeout` or a 10-minute ceiling through stream completion. Oversized dependency graphs are rejected [zkochan/pnpm-tasks#76](https://github.com/zkochan/pnpm-tasks/issues/76).
