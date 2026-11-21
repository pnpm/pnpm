---
"pacquet": minor
---

`pnpm pack` and `pnpm publish` now fail if a package that is not private lists a private workspace package in `dependencies`. A private package is never published, so the published package could not be installed. `devDependencies`, `optionalDependencies`, `peerDependencies`, and bundled dependencies are still allowed [pnpm/pnpm#2321](https://github.com/pnpm/pnpm/issues/2321).
