---
"pnpm": patch
---

The `pnpm config set` command should change the global `.npmrc` file by default.
This was a regression introduced by [#9151](https://github.com/pnpm/pnpm/pull/9151) and shipped in pnpm v10.5.0.
