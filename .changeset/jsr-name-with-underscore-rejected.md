---
"pnpm": patch
---

pnpm now rejects a `jsr:` specifier whose scope or package name contains an underscore. JSR names never contain one. `jsr:@a_/b` and `jsr:@a/_b` used to resolve to the same npm package, `@jsr/a___b`.
