---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

When a user runs `pnpm update` on a dependency that is not directly listed in `package.json`, none of the direct dependencies should be updated [#10155](https://github.com/pnpm/pnpm/pull/10155).
