---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

pnpm now deduplicates a package whose child dependency resolved an optional peer in one workspace project but not in another. Two copies of `next` could appear when only some projects could reach `styled-jsx`'s optional `babel-plugin-macros` peer [#14800](https://github.com/pnpm/pnpm/issues/14800).
