---
"pacquet": minor
---

Every context-aware global command (`node`, `deno`, `bun`, and the shims created with `pnpm shim add`) is now a native executable on every platform, so environment variables whose names are not valid shell identifiers reach these commands. On Windows, `<name>.exe` replaces the `.cmd` and `.ps1` shims for them. Shims written by earlier pnpm 12 releases are migrated on the next global install or self-update.
