---
"@pnpm/plugin-commands-config": major
"pnpm": major
---

`pnpm config get` (without `--json`) no longer print INI formatted text.
Instead, it would print JSON for both objects and arrays and raw string for
strings, numbers, booleans, and nulls.
`pnpm config get --json` would still print all types of values as JSON like before.
