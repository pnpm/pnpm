---
"pacquet": minor
---

Added `pnpm catalog migrate`, which moves dependency specifiers into the default catalog and replaces them in the workspace projects' `package.json` files with `catalog:`. A dependency moves only when every project declares it with the same specifier. The command lists the dependencies it leaves unchanged [#7072](https://github.com/pnpm/pnpm/issues/7072).
