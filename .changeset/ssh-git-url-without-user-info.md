---
"@pnpm/resolving.git-resolver": patch
"pnpm": patch
"pacquet": patch
---

An `ssh://` git dependency pointing at a bracketed IPv6 host, such as `ssh://[::1]/repo.git`, is resolved now. Its colons were read as an SCP-style path separator, which turned the address into `[:/1]` and left the specifier unresolvable. Applies to both the TypeScript CLI and pacquet.

In the TypeScript CLI, an `ssh://` git dependency written without user info — `ssh://git.example.com/team/repo.git`, `git+ssh://git.example.com:2222/team/repo.git` — no longer fails with `TypeError: Cannot read properties of undefined (reading 'includes')`. Only the `user@host` form worked before.
