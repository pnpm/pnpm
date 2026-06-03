---
"@pnpm/pnpr.client": minor
"@pnpm/config.reader": minor
"@pnpm/types": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.commands": patch
"pnpm": minor
---

Renamed the experimental `agent` setting to `pnprServer` so the pnpm CLI matches the same setting name pacquet uses for offloading resolution to a [pnpr](https://github.com/pnpm/pnpm/tree/main/pnpr) server. Point pnpm at a pnpr server with `pnprServer: <url>` in `pnpm-workspace.yaml` (or `--pnpr-server <url>`); the previous `agent` / `--agent` name no longer works. The client package was likewise renamed from `@pnpm/agent.client` to `@pnpm/pnpr.client`.
