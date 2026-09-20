---
"@pnpm/lockfile.preferred-versions": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fixed `pnpm dedupe` requiring a second pass after bumping a direct dependency in `package.json` [pnpm/pnpm#14987](https://github.com/pnpm/pnpm/issues/14987).
