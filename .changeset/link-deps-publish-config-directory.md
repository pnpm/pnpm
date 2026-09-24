---
"@pnpm/bins.linker": patch
"@pnpm/fs.symlink-dependency": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.direct-dep-linker": patch
"@pnpm/lockfile.filtering": patch
"@pnpm/resolving.local-resolver": patch
"@pnpm/workspace.project-manifest-reader": patch
"pnpm": patch
"pacquet": patch
---

Dependencies and executable binaries are now correctly linked and accessible for workspace packages using `publishConfig.directory` and `publishConfig.linkDirectory` [pnpm/pnpm#8338](https://github.com/pnpm/pnpm/issues/8338).
