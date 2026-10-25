---
"pacquet": minor
---

pnpm can now update a `pnpm-lock.yaml` that is a symlink. List the directories the lockfile may link into in `allowSymlinkedLockfile`, in the global config or with `PNPM_CONFIG_ALLOW_SYMLINKED_LOCKFILE`. pnpm then writes lockfile updates to the file the symlink points to if that file is inside one of them, and keeps the symlink in place. pnpm ignores this setting in `pnpm-workspace.yaml` [#17222](https://github.com/pnpm/pnpm/issues/17222).
