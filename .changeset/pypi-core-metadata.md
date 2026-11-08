---
"@pnpm/pnpr": patch
---

The Python registry proxy now supports core-metadata declarations. It caches `.metadata` downloads. pip and uv can read dependency metadata without downloading whole wheels.
