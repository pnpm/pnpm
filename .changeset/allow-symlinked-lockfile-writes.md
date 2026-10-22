---
"pacquet": minor
---

pnpm can now update a `pnpm-lock.yaml` that is a symlink. Set `allowSymlinkedLockfile` to `true` in the global config or with `PNPM_CONFIG_ALLOW_SYMLINKED_LOCKFILE=true`, and pnpm writes lockfile updates to the file the symlink points to and keeps the symlink in place. pnpm ignores this setting in `pnpm-workspace.yaml` [#17222](https://github.com/pnpm/pnpm/issues/17222).
