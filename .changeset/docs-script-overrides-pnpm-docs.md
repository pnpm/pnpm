---
"pnpm": minor
---

`pnpm docs` now runs a `docs` script from `package.json` when one exists, and passes it the positional arguments. `pnpm home` does the same for a `home` script. Run `pnpm pm docs` or `pnpm pm home` for the built-in command [#18516](https://github.com/pnpm/pnpm/issues/18516).
