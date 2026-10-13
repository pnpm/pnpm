---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now pack only the always-included files when `"files"` is `[]`. Those are `package.json`, the README, the license, and the `main` and `bin` targets. An empty `files` array used to pack every file in the package [pnpm/pnpm#16889](https://github.com/pnpm/pnpm/issues/16889).
