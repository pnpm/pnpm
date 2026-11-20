---
"@pnpm/fs.packlist": patch
"pnpm": patch
"pacquet": patch
---

Installing an injected local package no longer fails with a "no such file or directory" error when another process renames or removes files in that package while pnpm lists them. This happened most often on macOS in workspaces with `sharedWorkspaceLockfile: false`, where the install of the package itself writes its lockfile at the same time.
