---
"pacquet": patch
"pnpm": patch
---

pnpm no longer creates a project `pnpm-lock.yaml` when `devEngines.packageManager.onFail` is `download` and lockfile writing is turned off with `lockfile: false` or `--no-lockfile`. pnpm still switches to the pinned version [#14728](https://github.com/pnpm/pnpm/issues/14728).
