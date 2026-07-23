---
"@pnpm/installing.linking.hoist": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

Fixed resolution of implicit/phantom devDependencies under `enableGlobalVirtualStore: true` by linking hoisted dependencies into package virtual store directories across fresh and frozen installs.
