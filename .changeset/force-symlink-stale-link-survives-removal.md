---
"pacquet": patch
"@pnpm/napi": patch
---

A dependency symlink that pnpm cannot retarget, because the filesystem still serves the old link after pnpm removed it, now fails the install with an error that names the link. pnpm used to remove the link again without end and crashed with a segmentation fault once the thread ran out of stack. This happened on a `node_modules` served by a virtual filesystem.
