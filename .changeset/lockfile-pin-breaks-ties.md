---
"@pnpm/resolving.resolver-base": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/lockfile.preferred-versions": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

When `pnpm install` resolves a dependency again, it now reuses a version that the install already resolved for the same package instead of a different version the lockfile pinned. The lockfile result then matches a fresh install, without a second copy of the package. A locked version still stays when nothing else in the install prefers another version. To pin a transitive dependency on purpose, use `overrides` [#17084](https://github.com/pnpm/pnpm/issues/17084).
