---
"pacquet": patch
---

`pnpm update` now keeps the operator of every Python requirement, so `~=1.0` is updated to `~=2.0`. The `savePrefix` and `saveExact` settings no longer apply to Python requirements. A `savePrefix` such as `^` made `pnpm update` fail in a project with Python dependencies.
