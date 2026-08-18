---
"pacquet": patch
---

Fixed `file:` dependencies not being re-copied when their source directory changed. A `file:` dependency is copied into the store at install time rather than symlinked, so editing the local package's files and running `pnpm install` again left the previous copy in place — the lockfile is unchanged by such an edit, so the install treated the tree as up to date.
