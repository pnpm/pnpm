---
"@pnpm/global.commands": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm add -g` now installs each space-separated package into its own isolated directory by default. To bundle multiple packages into the same isolated install (so that they share dependencies and are removed together), pass them as a comma-separated list. For example:

- `pnpm add -g foo bar` installs `foo` and `bar` as two independent globals — removing one does not affect the other.
- `pnpm add -g foo,bar qar` bundles `foo` and `bar` into a single isolated install while `qar` is installed on its own.

Related: [#11587](https://github.com/pnpm/pnpm/issues/11587).
