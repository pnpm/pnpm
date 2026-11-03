---
"pacquet": patch
---

`pnpm update` no longer applies the `savePrefix` and `saveExact` settings to Python requirements. Each requirement keeps its operator, so `~=1.0` is updated to `~=2.0`. A `savePrefix` such as `^` made `pnpm update` fail in a project with Python dependencies.
