---
"pacquet": minor
---

Added the `allowSymlinkedLockfile` setting. When it is `true` and `pnpm-lock.yaml` is a symlink, pnpm writes lockfile updates to the file the symlink points to and keeps the symlink in place. The setting is read from the global config and from `PNPM_CONFIG_ALLOW_SYMLINKED_LOCKFILE`, not from `pnpm-workspace.yaml` [#17222](https://github.com/pnpm/pnpm/issues/17222).
