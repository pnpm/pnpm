---
"@pnpm/resolving.registry.pkg-metadata-filter": minor
"@pnpm/resolving.resolver-base": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/store.controller-types": minor
"@pnpm/installing.deps-resolver": minor
"pnpm": minor
"pacquet": minor
---

`minimumReleaseAge` now backs off to an older version of a dependency when the newer one cannot be installed under the cutoff. Previously the age check was applied to one package at a time, so a dependency that pins a package published minutes ago — most often a parent whose platform binaries were not all published at the same moment — failed the install with `ERR_PNPM_NO_MATURE_MATCHING_VERSION` even when an earlier version of that parent, and everything it depends on, was old enough. The install now resolves again with the offending version excluded and reports which versions it held back. The error, when no version works, names the dependent that required the immature package [#11068](https://github.com/pnpm/pnpm/issues/11068).
