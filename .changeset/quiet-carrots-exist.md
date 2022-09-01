---
"@pnpm/npm-resolver": patch
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Don't modify the manifest of the injected workspace project, when it has the same dependency in prod and peer dependencies.
