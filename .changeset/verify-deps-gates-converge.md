---
"pacquet": patch
---

`pnpm run` and `pnpm exec` no longer install before every command in a workspace without a root `package.json`.

With `sharedWorkspaceLockfile: false`, a filtered `pnpm run` or `pnpm exec` now also checks the workspace projects that the selected projects depend on.

When the install that runs before a script leaves the dependencies out of date, pnpm now prints a warning.
