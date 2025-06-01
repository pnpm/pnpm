---
"@pnpm/resolve-dependencies": patch
"@pnpm/package-requester": patch
"@pnpm/store-controller-types": patch
"@pnpm/core": patch
pnpm: patch
---

Fix a regression (in v10.9.0) causing the `--lockfile-only` flag on `pnpm update` to produce a different `pnpm-lock.yaml` than an update without the flag.
