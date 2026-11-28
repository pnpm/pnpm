---
"pnpm": patch
---

pnpm now rejects a `jsr:` specifier whose scope or package name contains an underscore. Before, `jsr:@a_/b` and `jsr:@a/_b` both resolved to the npm package `@jsr/a___b`.
