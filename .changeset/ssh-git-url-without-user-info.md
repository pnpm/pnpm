---
"@pnpm/resolving.git-resolver": patch
"pnpm": patch
---

An `ssh://` git dependency written without user info no longer fails with `TypeError: Cannot read properties of undefined (reading 'includes')`. Specifiers such as `ssh://git.example.com/team/repo.git` and `git+ssh://git.example.com:2222/team/repo.git` are resolved again; only the `user@host` form worked before.
