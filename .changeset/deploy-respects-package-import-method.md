---
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/releasing.commands": patch
"@pnpm/store.create-cafs-store": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now respects `--package-import-method` passed on the command line and reports the package import method correctly [pnpm/pnpm#7593](https://github.com/pnpm/pnpm/issues/7593).
