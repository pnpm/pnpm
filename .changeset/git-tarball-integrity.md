---
"@pnpm/building.after-install": patch
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.package-requester": patch
"@pnpm/modules-mounter.daemon": patch
"@pnpm/store.commands": patch
"@pnpm/store.pkg-finder": patch
"pnpm": patch
---

Pin the integrity of git-hosted tarballs (codeload.github.com, gitlab.com, bitbucket.org) in the lockfile so that subsequent installs detect a tampered or substituted tarball and refuse to install it. Previously the lockfile only stored the tarball URL for git dependencies, so a compromised git host or a man-in-the-middle could serve arbitrary code on later installs without lockfile changes.
