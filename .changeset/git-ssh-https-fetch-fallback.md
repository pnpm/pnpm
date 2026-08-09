---
"@pnpm/resolving.git-resolver": patch
"@pnpm/fetching.git-fetcher": patch
"pacquet": patch
"pnpm": patch
---

A git dependency whose lockfile entry records an SSH URL (`git@github.com:user/repo.git`) is now retried over HTTPS when the SSH clone fails, so an existing lockfile no longer breaks installs on machines without an SSH key, such as CI runners [#13743](https://github.com/pnpm/pnpm/issues/13743).
