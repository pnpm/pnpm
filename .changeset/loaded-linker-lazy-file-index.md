---
"pacquet": patch
---

With `nodeLinker.type: loaded`, Node.js processes start faster. The loader now indexes a package's files the first time that package is used. In a project with 13,000 stored files, the startup overhead per process dropped from 67 ms to 18 ms.
