---
"@pnpm/config.package-is-installable": patch
"pnpm": patch
---

`pnpm install --node-linker=hoisted` does not download skipped optional dependencies on repeat install.
