---
"@pnpm/patching.commands": patch
"pnpm": patch
---

Fixed `pnpm patch` dropping the package name (and leaking internal option fields) when the patched dependency resolves to a single git-hosted version.
