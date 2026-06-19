---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Install direct `file:` directory dependencies as projects during `pnpm install`, so their own dependencies are installed in the source directory too.
