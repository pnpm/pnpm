---
"pacquet": patch
---

When no directory above the project accepts a hard link — inside an AI agent sandbox that only grants write access to the project, or a container with just the project mounted writable — the default store is now created at `<project>/node_modules/.pnpm-store` instead of in the pnpm home directory. In those environments the home store is either read-only or on another volume, which forces every package to be copied instead of hard linked [#13525](https://github.com/pnpm/pnpm/issues/13525).
