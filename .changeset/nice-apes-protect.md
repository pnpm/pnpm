---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

Set `reporter-hide-prefix` to `true` by default for `pnpm exec`. In order to show prefix, the user now has to explicitly set `reporter-hide-prefix=false` [#8174](https://github.com/pnpm/pnpm/issues/8174).
