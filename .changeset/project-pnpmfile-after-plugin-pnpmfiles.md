---
"@pnpm/hooks.pnpmfile": patch
"pnpm": patch
---

The project's `.pnpmfile.mjs` or `.pnpmfile.cjs` now runs after the pnpmfiles of config dependency plugins, so its `updateConfig` hook can extend or override the settings a plugin sets [#9891](https://github.com/pnpm/pnpm/issues/9891).
