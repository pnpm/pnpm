---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Install in-project direct `file:` directory dependencies as projects during `pnpm install`, so their dependencies are installed in their directory [pnpm/pnpm#920](https://github.com/pnpm/pnpm/issues/920).
