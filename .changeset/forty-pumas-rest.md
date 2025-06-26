---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fix a deadlock that sometimes happens during peer dependency resolution [#9673](https://github.com/pnpm/pnpm/issues/9673).
