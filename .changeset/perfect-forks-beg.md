---
"@pnpm/core": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile --lockfile-only` should fail if the lockfile is not up to date with the `package.json` files [#6913](https://github.com/pnpm/pnpm/issues/6913).
