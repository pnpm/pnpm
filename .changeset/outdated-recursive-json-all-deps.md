---
"@pnpm/deps.inspection.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -r outdated --json` now includes every outdated workspace dependency when multiple projects depend on different versions or dependency types of the same package [#7693](https://github.com/pnpm/pnpm/issues/7693).
