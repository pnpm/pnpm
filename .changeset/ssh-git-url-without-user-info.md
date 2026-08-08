---
"@pnpm/resolving.git-resolver": patch
"pnpm": patch
---

An `ssh://` git dependency written without user info no longer fails with `TypeError: Cannot read properties of undefined (reading 'includes')`. Specifiers such as `ssh://git.example.com/team/repo.git` and `git+ssh://git.example.com:2222/team/repo.git` are resolved again; only the `user@host` form worked before.

An `ssh://` git dependency pointing at a bracketed IPv6 host, such as `ssh://[::1]/repo.git`, is also resolved now. Its colons were read as an SCP-style path separator, which turned the address into `[:/1]` and left `TypeError: Invalid URL`.
