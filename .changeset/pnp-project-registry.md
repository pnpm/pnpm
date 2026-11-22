---
"@pnpm/lockfile.to-pnp": patch
"pnpm": patch
---

Fixed Plug'n'Play installs with local `file:` directory dependencies. Root projects can resolve their dependencies, and workspace projects with duplicate names retain their own dependencies.
