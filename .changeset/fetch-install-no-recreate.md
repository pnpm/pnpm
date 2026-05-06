---
"@pnpm/installing.modules-yaml": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fix `pnpm install` recreating `node_modules` after `pnpm fetch`. `pnpm fetch` records empty `hoistPattern` and `publicHoistPattern` in `.modules.yaml`; since v11 removed the explicit-config gate, the follow-up install treated those as a hoist-pattern change and purged the modules directory. The fetch step now flags the modules manifest with `virtualStoreOnly: true` so the next install skips the hoist-pattern comparison and completes the missing post-import linking in place [#11488](https://github.com/pnpm/pnpm/issues/11488).
