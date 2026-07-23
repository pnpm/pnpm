---
"pacquet": patch
---

Fix `pnpm self-update <dist-tag>` recording the dist-tag (e.g. `next-12`) as the `packageManagerDependencies` specifier in `pnpm-lock.yaml`. It now records the resolved `devEngines.packageManager` pin, matching the manifest, so a later `--frozen-lockfile` install no longer fails with "the lockfile is not up to date".
