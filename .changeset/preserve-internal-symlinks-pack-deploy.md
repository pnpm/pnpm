---
"@pnpm/fs.packlist": patch
"@pnpm/releasing.commands": patch
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm pack`, `pnpm deploy`, and installs of local directory dependencies now keep symlinks that point to files or directories included in the package. `pnpm pack` leaves out symlinks that point outside the package [#8208](https://github.com/pnpm/pnpm/issues/8208).
