---
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm config` commands targeting global configuration to skip project package manager version switching, allowing registry authentication to be configured before pnpm downloads a project-pinned version [pnpm/pnpm#14463](https://github.com/pnpm/pnpm/issues/14463).
