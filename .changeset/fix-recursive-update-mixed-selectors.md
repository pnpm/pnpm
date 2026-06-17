---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fix recursive updates of transitive dependencies when the update command mixes transitive dependency patterns with direct dependency selectors. For example, `pnpm up -r "@babel/core" uuid` now updates matching transitive `@babel/core` dependencies even when `uuid` is a direct dependency selector [#12103](https://github.com/pnpm/pnpm/issues/12103).
