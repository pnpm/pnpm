---
"pacquet": minor
---

pnpm can now update a `pnpm-lock.yaml` that is a symlink. List the directories the lockfile may link into in the `symlinkedLockfileTargetDirs` setting, in the global config or with `PNPM_CONFIG_SYMLINKED_LOCKFILE_TARGET_DIRS`. pnpm writes the update to the file the symlink points to if that file is inside one of these directories. The symlink stays in place. pnpm ignores this setting in `pnpm-workspace.yaml` [#17222](https://github.com/pnpm/pnpm/issues/17222).
