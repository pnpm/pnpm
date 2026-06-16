---
"@pnpm/bins.linker": patch
"pnpm": patch
---

Skip the redundant "target bin directory already contains an exe called node" warning on Windows when the existing `node.exe` already matches the target (same hard link or identical content) [pnpm/pnpm#12203](https://github.com/pnpm/pnpm/issues/12203).
