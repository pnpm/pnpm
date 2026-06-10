---
"@pnpm/types": patch
"@pnpm/dependency-path": patch
"@pnpm/builder.policy": patch
"@pnpm/lockfile.utils": patch
"@pnpm/lockfile.fs": patch
"@pnpm/build-modules": patch
"@pnpm/prepare-package": patch
"@pnpm/exec.build-commands": patch
"@pnpm/plugin-commands-rebuild": patch
"@pnpm/git-fetcher": patch
"@pnpm/tarball-fetcher": patch
"@pnpm/git-resolver": patch
"@pnpm/core": patch
"@pnpm/headless": patch
"pnpm": patch
---

Require trusted package identity before package-name `onlyBuiltDependencies` (and `allowBuilds`) entries can approve lifecycle scripts for git, git-hosted tarball, direct tarball, and local directory artifacts. To approve one of those artifacts explicitly, use its peer-suffix-free lockfile depPath as the key. Lockfile entries are now rejected when a registry-style dependency path (`name@semver`) is backed by a git, directory, or git-hosted tarball resolution (`ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`), so the dependency path is a reliable artifact identity by the time scripts can run.
