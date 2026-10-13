---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now pack only `package.json`, the README, the license, and the files named in `main` and `bin` when `files` is an empty array. They used to pack every file in the package [#16889](https://github.com/pnpm/pnpm/issues/16889).
