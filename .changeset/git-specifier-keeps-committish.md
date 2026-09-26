---
"@pnpm/resolving.git-resolver": patch
"pnpm": patch
"pacquet": patch
---

A git dependency installed over HTTPS from a hosted repository now keeps its branch, tag, or version range in the specifier recorded in `package.json`. It was written back without one, so the next `pnpm update` moved the dependency to the repository's default branch [#13999](https://github.com/pnpm/pnpm/issues/13999).
