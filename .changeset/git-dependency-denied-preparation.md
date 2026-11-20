---
"@pnpm/exec.prepare-package": patch
"@pnpm/fetching.fetcher-base": patch
"@pnpm/fetching.git-fetcher": patch
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/installing.package-requester": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now installs git-hosted dependencies without preparing them when their builds are explicitly denied by `allowBuilds`. Dependencies that require preparation still need an explicit allow or deny decision [pnpm/pnpm#10522](https://github.com/pnpm/pnpm/issues/10522).
