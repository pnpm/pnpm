---
"@pnpm/symlink-dependency": patch
"pnpm": patch
---

When dealing with a local dependency that is a path to a symlink, a new symlink should be created to the original symlink, not to the actual directory location.
