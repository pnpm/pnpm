---
"@pnpm/hooks.read-package-hook": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

`pnpm update` no longer moves the range a project declares for a dependency that `overrides` also lists, even when the override repeats that range verbatim. Previously the updated `package.json` disagreed with the lockfile, so the next `pnpm install --frozen-lockfile` failed with a specifier mismatch [#14224](https://github.com/pnpm/pnpm/issues/14224).
