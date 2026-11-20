---
"@pnpm/installing.deps-resolver": minor
"pnpm": minor
"pacquet": minor
---

Optional peer dependencies declared only via `peerDependenciesMeta` (for example `debug`'s `supports-color` peer) are now resolved from a satisfying version already present in the dependency graph, the same way explicitly declared optional peer dependencies are. Previously such peers were only resolved this way when the package's metadata was read back from the lockfile, so an unrelated dependency change could rewrite peer resolutions across the whole lockfile.
