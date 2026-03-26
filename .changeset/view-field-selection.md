---
"@pnpm/deps.inspection.commands": minor
"pnpm": minor
---

Added field selection support to `pnpm view` command, allowing users to query specific fields from package registry information (e.g., `pnpm view react version`, `pnpm view react@18 dependencies`, `pnpm view lodash dist.tarball`). This matches npm's view command behavior.
