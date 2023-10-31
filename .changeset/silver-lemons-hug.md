---
"@pnpm/resolve-dependencies": patch
"@pnpm/package-requester": patch
"@pnpm/store-controller-types": patch
"@pnpm/core": patch
"pnpm": patch
---

If a package's tarball cannot be fetched, print the dependency chain that leads to the failed package [#7265](https://github.com/pnpm/pnpm/pull/7265).
