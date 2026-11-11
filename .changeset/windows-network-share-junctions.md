---
"pacquet": patch
---

On Windows, `pnpm install` now links dependencies in `node_modules` with junctions when the project is on a network share, even when symlinks are allowed. By default, Windows does not follow a symlink on a share that points back into a share, so packages linked that way could not be loaded.
