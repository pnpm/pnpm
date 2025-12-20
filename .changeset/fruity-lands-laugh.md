---
"@pnpm/fs.hard-link-dir": patch
"pnpm": patch
---

Don't crash when two processes of pnpm are hardlinking the contents of a directory to the same destination simultaneously [#10179](https://github.com/pnpm/pnpm/issues/10179).
