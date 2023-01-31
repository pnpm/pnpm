---
"@pnpm/resolve-dependencies": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

A new `resolution-mode` added: `lowest-direct`. With this resolution mode direct dependencies will be resolved to their lowest versions. So if there is `foo@^1.1.0` in the dependencies, then `1.1.0` will be installed, even if the latest version of `foo` is `1.2.0`.
