---
pnpm: patch
---

Fix `pnpm --version` hanging for the lifetime of the worker pool after the version was printed. `main.ts`'s `--version` short-circuit returned before reaching the command-handler `finally` that calls `finishWorkers()`, so the worker pool that `switchCliVersion` had spawned during integrity resolution stayed alive and held the Node event loop open. The CLI entry now runs `finishWorkers()` from its own `finally`, so every exit path tears the pool down.

Repro: `pnpm --version` in a workspace whose `devEngines.packageManager` version already matches the running pnpm + `onFail: "download"`. `switchCliVersion` resolves the integrity (spawning workers), finds nothing to swap, returns. The version prints, then the process hangs.
