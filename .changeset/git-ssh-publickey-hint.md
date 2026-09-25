---
"@pnpm/fetching.git-fetcher": patch
"@pnpm/resolving.git-resolver": patch
"pacquet": patch
"pnpm": patch
---

When installing a git dependency over SSH fails with `Permission denied (publickey)`, pnpm says to load a key with `ssh-add -l`.

Resolving an SSH URL that refuses the key also shows a local HTTPS rewrite that leaves the recorded URL alone [pnpm/pnpm#13743](https://github.com/pnpm/pnpm/issues/13743).
