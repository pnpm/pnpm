---
"@pnpm/fs.packlist": patch
"pnpm": patch
---

`pnpm pack` and `pnpm publish` include files from a publish directory reached through a symlink [pnpm/pnpm#17390](https://github.com/pnpm/pnpm/issues/17390).
