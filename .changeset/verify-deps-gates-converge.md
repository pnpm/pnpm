---
"pacquet": patch
---

In a workspace that has no `package.json` at its root, `pnpm run` and `pnpm exec` no longer reinstall before every command when the dependencies are up to date.

With `sharedWorkspaceLockfile: false`, a filtered `pnpm run` or `pnpm exec` now also checks the workspace projects that the selected projects depend on.

When the install that runs before a script leaves the dependencies out of date, pnpm now prints a warning.
