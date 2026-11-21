---
"pnpm": patch
---

Fixed `nodeLinker: hoisted` installs placing a package in a directory from which one of its dependencies resolved to another version than the lockfile records. It could happen in a workspace whose projects depend on different versions of the same packages.
