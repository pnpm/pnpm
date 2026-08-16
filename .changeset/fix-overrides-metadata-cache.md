---
"@pnpm/hooks.read-package-hook": patch
"pnpm": patch
---

Fixed an issue where package overrides were written into the metadata cache, causing removed overrides to keep applying on subsequent installs [pnpm/pnpm#13918](https://github.com/pnpm/pnpm/issues/13918).
