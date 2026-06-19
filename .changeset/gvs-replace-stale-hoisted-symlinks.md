---
"@pnpm/installing.linking.hoist": patch
"pnpm": patch
---

When `enable-global-virtual-store` is toggled on for a project that was previously installed without it, stale hoisted symlinks under `node_modules/.pnpm/node_modules` are now replaced instead of being left pointing at the old per-project virtual store location [#9739](https://github.com/pnpm/pnpm/issues/9739).
