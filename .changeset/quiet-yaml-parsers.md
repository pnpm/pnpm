---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

pnpm now limits YAML merge work when reading lockfiles and uses a parser with a fix for ordered maps. A lockfile that exceeds 10,000 combined merge sources and merged keys per document must be regenerated [#18246](https://github.com/pnpm/pnpm/issues/18246).
