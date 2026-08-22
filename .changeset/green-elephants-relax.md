---
"@pnpm/installing.commands": patch
"pnpm": patch
---

Fix recursive `pnpm update <name>@<version> --lockfile-only --no-save` so exact pinned updates stay scoped to the requested version line and do not update other major versions of the same package.
