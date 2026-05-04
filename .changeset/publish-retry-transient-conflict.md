---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm publish` now automatically retries with exponential backoff when the npm registry responds with a transient `409 Conflict` ("Failed to save packument."). This error happens when a publish is attempted while a previous write for the same package is still being processed by the registry — for example, when running `pnpm publish` shortly after `pnpm publish --dry-run` [#11454](https://github.com/pnpm/pnpm/issues/11454).
