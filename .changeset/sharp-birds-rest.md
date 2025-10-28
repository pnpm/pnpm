---
"@pnpm/plugin-commands-config": major
"pnpm": major
---

`pnpm config list` and `pnpm config get` (without argument) now show top-level keys as camelCase.
Exception: Keys that start with `@` or `//` would be preserved (their cases don't change).
