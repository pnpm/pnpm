---
"@pnpm/building.after-install": patch
"@pnpm/building.during-install": patch
"@pnpm/building.policy": patch
"@pnpm/deps.graph-builder": patch
"@pnpm/deps.graph-hasher": patch
"@pnpm/exec.prepare-package": patch
"@pnpm/fetching.git-fetcher": patch
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/types": patch
"pnpm": patch
---

Require trusted package identity before package-name `allowBuilds` entries can approve lifecycle scripts for git, git-hosted tarball, and direct tarball artifacts. To approve one of those artifacts explicitly, use its peer-suffix-free lockfile depPath as the `allowBuilds` key. Lockfile verification now rejects lockfiles where a registry-style dependency path (`name@semver`) is backed by a git, directory, or git-hosted tarball resolution (`ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`), so the dependency path is a reliable artifact identity by the time scripts can run.
