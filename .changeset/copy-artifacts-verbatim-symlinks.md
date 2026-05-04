---
"@pnpm/exe": patch
---

Also pass `verbatimSymlinks: true` to the `fs.cpSync` call in `__utils__/scripts/src/copy-artifacts.ts`, which is the script that actually produces the GitHub release tarballs (`pnpm-{darwin,linux}-{x64,arm64}.tar.gz`). The previous fix in #11399 only covered the `fs.cpSync` in `pnpm/artifacts/exe/scripts/build-artifacts.ts`, which packages the `dist/` shipped inside the npm-published `@pnpm/exe` package. Verified by inspecting the v11.0.2 release tarballs after #11399 landed: the broken `/home/runner/work/pnpm/pnpm/...` symlinks under `dist/node_modules/.bin/` were still present, confirming `copy-artifacts.ts` is the offender for the GitHub release path. Follow-up to #11398.
