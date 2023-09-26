---
"@pnpm/plugin-commands-listing": patch
"@pnpm/reviewing.dependencies-hierarchy": patch
---

Fix memory error when using `pnpm why <package>` in a project with many dependencies, the result is cropped to 10 end leafs and now supports the option to limit the depth (`pnpm why <package> --depth 2`).
