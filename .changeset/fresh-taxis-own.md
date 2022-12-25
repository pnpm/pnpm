---
"@pnpm/prepare-package": major
"@pnpm/git-fetcher": major
"@pnpm/tarball-fetcher": major
"@pnpm/client": major
---

A new required option added to the prepare package function: rawConfig. It is needed in order to create a proper environment for the package manager executed during the preparation of a git-hosted dependency.
