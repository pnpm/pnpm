---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

`pnpm update --latest !foo` should not update anything if the only dependency in the project is the ignored one [#5643](https://github.com/pnpm/pnpm/pull/5643).
