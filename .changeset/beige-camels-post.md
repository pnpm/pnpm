---
"@pnpm/headless": patch
"@pnpm/deps.graph-builder": patch
"@pnpm/core": patch
pnpm: patch
---

Fix an edge case bug causing local tarballs to not re-link into the virtual store. This bug would happen when changing the contents of the tarball without renaming the file and running a filtered install.
