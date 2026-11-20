---
"pacquet": patch
---

On Windows, when the project is on a network share, `pnpm install` now links dependencies in `node_modules` with the kind of link that can be opened on that share. It tries a junction first and a symlink second. If neither can be opened, the install fails with advice instead of finishing with packages that cannot be loaded.
