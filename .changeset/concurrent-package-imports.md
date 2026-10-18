---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
"pacquet": patch
---

Concurrent installs that import the same package into one project now all complete. One of them used to fail with `ENOTEMPTY` when its copy of the package collided with the copy another install had already placed [pnpm/tasks#48](https://github.com/pnpm/tasks/issues/48).
