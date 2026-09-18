---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm update` and `pnpm audit --fix=update` no longer copy dependencies added by `packageExtensions`, a `readPackage` hook, or an override into `package.json`. Those dependencies keep the specifier the hook or override gives them, and `pnpm update --latest` no longer resolves past it. When one of them pins a vulnerable version, `pnpm audit --fix=update` now reports that it cannot fix it and points at `pnpm audit --fix` [#14928](https://github.com/pnpm/pnpm/issues/14928).
