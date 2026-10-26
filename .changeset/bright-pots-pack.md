---
"@pnpm/fs.packlist": patch
"pnpm": patch
---

`pnpm pack` and `pnpm publish` again include the files of a `publishConfig.directory` that is reached through a symlink, together with its bundled dependencies [pnpm/pnpm#17390](https://github.com/pnpm/pnpm/issues/17390).
