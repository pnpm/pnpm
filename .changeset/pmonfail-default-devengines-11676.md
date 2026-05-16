---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fix `devEngines.packageManager` (singular form, without `onFail`) defaulting to `onFail: "error"` instead of the documented `pmOnFail: "download"`. As a result, a project that pinned a different pnpm version via `devEngines.packageManager` and ran `pnpm install` from a mismatched pnpm version failed with a hard error, even though the migration table from `managePackageManagerVersions: true` to `pmOnFail: download (default)` promises the install would auto-download the wanted version [#11676](https://github.com/pnpm/pnpm/issues/11676).

The array form of `devEngines.packageManager` keeps its existing per-element defaults (`error` for the last entry, `ignore` for the rest), since those reflect explicit prioritisation by the user. Explicit `onFail` values continue to win.
