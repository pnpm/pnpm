---
"@pnpm/link-bins": patch
"pnpm": patch
---

When linking commands to a directory, remove any .exe files that are already present in that target directory by the same name.

This fixes an issue with pnpm global update on Windows. If pnpm was installed with the standalone script and then updated with pnpm using `pnpm add --global pnpm`, the exe file initially created by the standalone script should be removed.
