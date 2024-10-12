---
"@pnpm/store.cafs": patch
"pnpm": patch
---

When checking whether a file in the store has executable permissions, the new approach checks if at least one of the executable bits (owner, group, and others) is set to 1. Previously, a file was incorrectly considered executable only when all the executable bits were set to 1. This fix ensures that files with any executable permission, regardless of the user class, are now correctly identified as executable [#8546](https://github.com/pnpm/pnpm/issues/8546).
