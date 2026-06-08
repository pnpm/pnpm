---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fixed bare `--color` so it does not consume the following CLI flag, allowing command shorthands like `--parallel` to expand correctly and forms like `pnpm --color with current <command>` to dispatch the inner command instead of failing with `MISSING_WITH_CURRENT_CMD`.
