---
"pnpm": minor
---

`pnpm pack` now accepts package specs, like `npm pack <spec>`. It packs a registry package (`pnpm pack foo@1.0.0`), a git repository (`pnpm pack github:user/repo`), a tarball URL (`pnpm pack https://pkg.pr.new/vite@24234`), a local directory, or a local tarball file. pnpm verifies a registry download against the integrity declared in the registry metadata.
