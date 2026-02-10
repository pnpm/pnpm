---
"@pnpm/package-bins": patch
"pnpm": patch
---

Fixed a path traversal vulnerability in pnpm's bin linking. Bin names starting with `@` bypassed validation, and after scope normalization, path traversal sequences like `../../` remained intact.