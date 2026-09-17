---
"pacquet": minor
---

Added the `machineRunConcurrency` setting. It caps how many `pnpm run`, `pnpm exec`, and script shortcut invocations such as `pnpm test` execute at once on a machine. An invocation past the limit waits for one of the running ones to finish and prints which processes hold the slots. A script that calls `pnpm run` itself runs under the slot its parent holds. The `machineRunConcurrencyGroup` setting names the pool of slots, so several workspaces can share one limit or keep their own. Set both in the global `config.yaml` to apply them to every workspace on the machine.
