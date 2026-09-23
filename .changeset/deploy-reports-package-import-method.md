---
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/releasing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm deploy` now reports the import method it used for packages from the store. It said "Packages are cloned from the content-addressable store" whatever `packageImportMethod` was set to [#7593](https://github.com/pnpm/pnpm/issues/7593).
