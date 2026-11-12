---
"pacquet": patch
---

`pnpm install` with `pnprServer` now bounds response sizes from `/-/pnpr/v0/resolve` and `/-/pnpr/v0/verify-lockfile`. Oversized NDJSON frames, excessive frame counts, and large error bodies abort the exchange cleanly [zkochan/pnpm-tasks#76](https://github.com/zkochan/pnpm-tasks/issues/76).
