---
"pacquet": patch
"pnpm": patch
---

`lockfile: false` and `--no-lockfile` are honored when `devEngines.packageManager.onFail` is `download`. pnpm still switches to the pinned version, and it no longer creates a project `pnpm-lock.yaml` for that pin [#14728](https://github.com/pnpm/pnpm/issues/14728).
