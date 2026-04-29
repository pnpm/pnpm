---
"@pnpm/exe": patch
---

Preserve relative symlinks under `dist/node_modules/.bin/` when copying `dist/` for the standalone executable artifact, by passing `verbatimSymlinks: true` to `fs.cpSync`. This stops the release tarballs from baking absolute paths to the build host (e.g. `/home/runner/work/pnpm/pnpm/...`) into symlink targets, which previously made the tarballs unextractable by strict tar implementations that validate symlink targets (e.g. hermit) [#11398](https://github.com/pnpm/pnpm/issues/11398).
