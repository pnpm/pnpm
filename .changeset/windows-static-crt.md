---
"pacquet": patch
---

`pnpm` and its Node.js addon now start on a Windows install without the Visual C++ Redistributable. They used to exit with `0xC0000135`, a missing DLL error [#15723](https://github.com/pnpm/pnpm/issues/15723).
