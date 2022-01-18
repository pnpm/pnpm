---
"@pnpm/core": minor
"@pnpm/headless": minor
"@pnpm/plugin-commands-installation": minor
"pnpm": minor
---

Side effects cache is not an experimental feature anymore.

Side effects cache is saved separately for packages with different dependencies. So if `foo` has `bar` in the dependencies, then a separate cache will be created each time `foo` is installed with a different version of `bar` [#4238](https://github.com/pnpm/pnpm/pull/4238).

