---
"@pnpm/directory-fetcher": patch
"pnpm": patch
---

Installation shouldn't fail when the injected dependency has broken symlinks. The broken symlinks should be just skipped [#5598](https://github.com/pnpm/pnpm/issues/5598).
