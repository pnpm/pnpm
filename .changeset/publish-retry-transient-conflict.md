---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm publish` now automatically retries with exponential backoff when the npm registry responds with a transient `409 Conflict` ("Failed to save packument."). The registry returns this when a publish lands while a previous write for the same package is still being processed [#11454](https://github.com/pnpm/pnpm/issues/11454).
