---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm update` and `pnpm audit --fix=update` no longer copy dependencies added by `packageExtensions`, a `readPackage` hook, or an override into `package.json`. Those dependencies keep the specifier the hook or override gives them. `pnpm update --latest` no longer resolves past that specifier. `pnpm audit --fix=update` now warns when one of them pins a vulnerable version. The warning points at `pnpm audit --fix` [#14928](https://github.com/pnpm/pnpm/issues/14928).
