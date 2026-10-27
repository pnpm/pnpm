---
"@pnpm/cli.parse-cli-args": patch
"@pnpm/config.reader": patch
"@pnpm/engine.pm.commands": patch
"@pnpm/exec.commands": patch
"@pnpm/workspace.workspace-manifest-reader": patch
"pacquet": patch
"pnpm": patch
---

Commands, `pnpm --version`, `pnpm self-update`, and invocations with unknown flags now apply the same `packageManager` and `devEngines.packageManager` settings, `--config.<key>` flags, and `lockfileDir` when switching to the pinned pnpm version or recording it in `pnpm-lock.yaml`, including when unrelated settings in `pnpm-workspace.yaml` or the global `config.yaml` are unreadable [pnpm/tasks#222](https://github.com/pnpm/tasks/issues/222).
