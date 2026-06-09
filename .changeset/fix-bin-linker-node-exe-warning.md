---
"@pnpm/bins.linker": patch
"pnpm": patch
---

Skip redundant "target bin directory already contains an exe called node" warning when the .exe is already the correct hardlink [pnpm/pnpm#12203](https://github.com/pnpm/pnpm/issues/12203).
