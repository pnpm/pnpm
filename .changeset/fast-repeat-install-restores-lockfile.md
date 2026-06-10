---
"@pnpm/deps.status": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

`pnpm install` completes without re-resolving when `pnpm-lock.yaml` was deleted but `node_modules` is intact: the up-to-date check now treats the current lockfile (`node_modules/.pnpm/lock.yaml`) — the record of what the previous install materialized — as the wanted lockfile, verifies the manifests still match it, restores `pnpm-lock.yaml` from it, and reports "Already up to date". Previously this scenario triggered a full resolution and a re-verification of every locked package against the registry.
