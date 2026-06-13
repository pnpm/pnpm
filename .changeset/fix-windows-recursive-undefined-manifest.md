---
"@pnpm/installing.commands": patch
"pnpm": patch
---

Fixed `Cannot destructure property 'manifest' of 'manifestsByPath[rootDir]' as it is undefined` regression introduced in 11.6.0 when running `pnpm add <pkg>` outside a workspace on Windows. `selectProjectByDir` was keying the resulting `ProjectsGraph` by `opts.dir` instead of `project.rootDir`, so downstream `manifestsByPath` lookups missed when the two paths normalized differently (typically drive-letter casing). [pnpm/pnpm#12379](https://github.com/pnpm/pnpm/issues/12379)
