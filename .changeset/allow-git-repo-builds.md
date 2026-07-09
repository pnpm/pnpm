---
"@pnpm/building.policy": patch
"pnpm": patch
---

Allow `allowBuilds` entries for git-hosted packages to match by repository URL without pinning the resolved commit hash. This lets trusted git repositories keep running their build scripts after branch updates without approving each new commit, while package-name-only rules still do not approve git-hosted artifacts.
