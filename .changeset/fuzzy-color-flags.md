---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fixed bare `--color` so it does not consume the following CLI flag, allowing command shorthands like `--parallel` to expand correctly.
