---
"pacquet": patch
---

`pnpm` starts faster on Windows. It loads system libraries such as `user32.dll` and `shell32.dll` the first time a command uses them, so `pnpm --version` loads 11 libraries where it loaded 25.
