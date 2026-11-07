---
"@pnpm/pnpr": patch
---

The Python registry proxy now preserves core-metadata declarations and caches `.metadata` downloads. pip and uv can read dependency metadata without downloading whole wheels.
