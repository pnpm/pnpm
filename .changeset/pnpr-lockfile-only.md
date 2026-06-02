---
"@pnpm/agent.client": minor
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install --lockfile-only` (and the `lockfileOnly` setting) is now honored when an agent / `pnprServer` is configured. The agent path resolves and writes `pnpm-lock.yaml` but fetches no files into the store and links no `node_modules`, matching the local lockfile-only behavior. The client ignores any file/index lines an older agent server still streams, so the store stays untouched even against a server that predates the resolve-only mode [#12146](https://github.com/pnpm/pnpm/issues/12146).
