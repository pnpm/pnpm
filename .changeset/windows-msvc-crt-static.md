---
"pacquet": patch
---

The Windows `pnpm.exe` runs on a clean Windows install that does not have the Visual C++ Redistributable. It used to exit immediately on startup because that runtime was missing [pnpm/pnpm#15723](https://github.com/pnpm/pnpm/issues/15723).
