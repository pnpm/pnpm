---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Installation shouldn't fail when a package from node_modules is moved to the `node_modules/.ignored` subfolder and a package with that name is already present in `node_modules/.ignored'.
