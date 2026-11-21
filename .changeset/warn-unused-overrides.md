---
"pacquet": minor
---

`pnpm install` now warns when an entry in `overrides` matches no dependency, in a single line such as `2 overrides matched no dependency: foo, parent>child`. Only the overrides declared in the workspace are checked. Overrides added by a config dependency or an `updateConfig` hook are not. Set `warnUnusedOverrides: false` to turn the warning off [#10315](https://github.com/pnpm/pnpm/issues/10315).
