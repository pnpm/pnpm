---
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/store.controller-types": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now preserves selected internal symlinks whose targets are installed dependencies or other files outside the copied file set. This applies to default and legacy deploys. Fixes [pnpm/pnpm#17288](https://github.com/pnpm/pnpm/issues/17288).
