---
"@pnpm/config.reader": minor
"@pnpm/installing.env-installer": minor
"pnpm": minor
---

When pnpm is declared via the `packageManager` field in `package.json`, its resolution info is no longer written to `pnpm-lock.yaml` — unless the pinned pnpm version is v12 or newer. The `packageManagerDependencies` section is still populated (and reused across runs) when pnpm is declared via `devEngines.packageManager`. This makes the transition from pnpm v10 to v11 quieter by avoiding unnecessary lockfile churn for projects that pin an older pnpm in the legacy `packageManager` field.
