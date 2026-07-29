---
"pacquet": patch
---

A project that pins pnpm through `devEngines.packageManager` (or a v12+ `packageManager` field) now gets its `packageManagerDependencies` recorded in `pnpm-lock.yaml` by every command, not just by the install-family ones [#13348](https://github.com/pnpm/pnpm/issues/13348). Running `pnpm list` (or any other command) in a freshly cloned project no longer leaves the lockfile without the pinned version. The `pmOnFail` setting now also decides whether the pin is recorded: `--pm-on-fail=ignore` keeps it out of the lockfile even when the manifest asks for a stricter policy, and vice versa.
