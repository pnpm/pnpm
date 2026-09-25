---
"@pnpm/patching.commands": patch
"pnpm": patch
---

`pnpm patch-commit` now fails with an error when `git` cannot be found in `PATH`. It previously reported that no changes were found [pnpm/pnpm#8666](https://github.com/pnpm/pnpm/issues/8666).
