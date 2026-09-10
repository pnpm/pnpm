---
"pacquet": patch
---

`pnpm install` and `pnpm dedupe` now prune the `minimumReleaseAgeExclude` and `trustPolicyExclude` entries in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves, when `minimumReleaseAgeExcludePrune` or `trustPolicyExcludePrune` is enabled. Only `pnpm add`, `pnpm update`, and `pnpm remove` ran that cleanup before [pnpm/pnpm#14759](https://github.com/pnpm/pnpm/issues/14759).
