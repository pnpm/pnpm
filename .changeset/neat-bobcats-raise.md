---
"@pnpm/plugin-commands-installation": minor
"@pnpm/config": minor
"pnpm": minor
---

Added a new setting called `optimistic-repeat-install`. When enabled, a fast check will be performed before proceeding to installation. This way a repeat install or an install on a project with everything up-to-date becomes a lot faster. But some edge cases might arise, so we keep it disabled by default for now [#8977](https://github.com/pnpm/pnpm/pull/8977).
