---
"@pnpm/config": patch
"@pnpm/normalize-registries": patch
---

When normalizing registry URLs, a trailing slash should only be added if the registry URL has no path.

So `https://registry.npmjs.org` is changed to `https://registry.npmjs.org/` but `https://npm.pkg.github.com/owner` is unchanged.

Related issue: [#4034](https://github.com/pnpm/pnpm/issues/4034).
