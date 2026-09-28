---
"pacquet": patch
---

`pnpm install` in a workspace with `sharedWorkspaceLockfile: false` now installs projects concurrently, up to `workspaceConcurrency` at a time. A project is resolved, fetched, and written to its virtual store without waiting for the workspace projects it depends on. It waits for them only before it links its dependencies and runs its lifecycle scripts, so its scripts still run after theirs. A project with a `preinstall` or `pnpm:devPreinstall` script, or with an injected or `file:` workspace dependency, waits for its workspace dependencies before it starts. An install with a pnpmfile no longer starts an extra Node.js process when the pnpmfile has no `preResolution` hook [#14480](https://github.com/pnpm/pnpm/issues/14480).
