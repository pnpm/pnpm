---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

pnpm can read lockfiles with large YAML merge mappings or ordered maps without the excessive parsing time seen in some cases [#18246](https://github.com/pnpm/pnpm/issues/18246).
