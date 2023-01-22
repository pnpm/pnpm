---
"@pnpm/modules-cleaner": patch
"@pnpm/core": patch
"pnpm": patch
---

Packages hoisted to the virtual store are not removed on repeat install, when the non-headless algorithm runs the installation.
