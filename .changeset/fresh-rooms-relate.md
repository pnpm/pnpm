---
"@pnpm/config": patch
"pnpm": patch
---

`pnpm install --prod=false` should not crash, when executed in a project with a `pnpm-workspace.yaml` file [#9233](https://github.com/pnpm/pnpm/issues/9233). This fixes regression introduced via [#9211](https://github.com/pnpm/pnpm/pull/9211).
