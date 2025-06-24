---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Running `pnpm install` after `pnpm fetch` should hoist all dependencies that need to be hoisted.
Fixes a regression introduced in [v10.12.2] by [#9648]; resolves [#9689].

[v10.12.2]: https://github.com/pnpm/pnpm/releases/tag/v10.12.2Add commentMore actions
[#9648]: https://github.com/pnpm/pnpm/pull/9648
[#9689]: https://github.com/pnpm/pnpm/issues/9689
