---
"@pnpm/fetching.binary-fetcher": patch
"pnpm": patch
---

Fix path traversal vulnerability in binary fetcher ZIP extraction

- Validate ZIP entry paths before extraction to prevent writing files outside target directory
- Validate BinaryResolution.prefix (basename) to prevent directory escape via crafted prefix
- Both attack vectors now throw `ERR_PNPM_PATH_TRAVERSAL` error
