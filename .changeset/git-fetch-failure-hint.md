---
"@pnpm/fetching.git-fetcher": patch
"@pnpm/error": patch
"pacquet": patch
"pnpm": patch
---

A git dependency whose clone (or shallow fetch) fails now reports which package it belongs to, under the `ERR_PNPM_GIT_FETCH_FAILED` code, with credentials in the repository URL redacted. When the lockfile records an SSH remote, the error also explains that fetching it needs an SSH key for that host, and that a lockfile entry written before pnpm v11.21 can be re-recorded over HTTPS with `pnpm update <package>` [#13743](https://github.com/pnpm/pnpm/issues/13743).
