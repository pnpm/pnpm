---
"pacquet": patch
---

`pnpm install` in a workspace with `sharedWorkspaceLockfile: false` now installs projects concurrently, up to `workspaceConcurrency` at a time. A project still waits for the workspace projects it depends on. An install with a pnpmfile no longer starts an extra Node.js process when the pnpmfile has no `preResolution` hook [#14480](https://github.com/pnpm/pnpm/issues/14480).
