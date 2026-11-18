---
"@pnpm/pnpr": patch
---

pnpr now returns freed memory to the operating system. On Linux, its resident memory grew to several GB while serving many large packuments at once and stayed there after the requests finished. pnpr also keeps more rendered packuments in its cache, so repeated reads of large packages need less memory and time [#18206](https://github.com/pnpm/pnpm/issues/18206).
