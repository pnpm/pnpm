---
"@pnpm/pnpr": patch
---

pnpr now returns freed memory to the operating system. On Linux, its resident memory grew to several GB while serving many large packuments at once and stayed there after the requests finished [#18206](https://github.com/pnpm/pnpm/issues/18206).
