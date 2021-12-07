---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

When `strict-peer-dependencies` is used, don't fail on the first peer dependency issue. Print all the peer dependency issues and then stop the installation process [#4082](https://github.com/pnpm/pnpm/pull/4082).
