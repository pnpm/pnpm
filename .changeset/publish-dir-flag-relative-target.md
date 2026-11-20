---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -C <dir> publish <tarball|folder>` and `pnpm --dir <dir> publish <tarball|folder>` now resolve relative tarball and directory arguments against `<dir>`. Git checks during `pnpm publish` also inspect the target directory.
