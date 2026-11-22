---
"pnpm": patch
---

`pnpm-lock.yaml` now quotes strings that look like numbers, such as `+.5`, so they are read back as strings.
