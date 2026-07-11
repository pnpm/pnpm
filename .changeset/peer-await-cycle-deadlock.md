---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed a deadlock in peer dependency resolution: `pnpm install` hung forever when a peer dependency cycle spanned a project's own dependencies and auto-installed peer providers, for example when installing `electron-builder@26.15.3` [#12921](https://github.com/pnpm/pnpm/issues/12921).
