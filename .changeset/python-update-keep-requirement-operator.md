---
"pacquet": patch
---

`pnpm update` no longer applies the `savePrefix` and `saveExact` settings to Python requirements. A `savePrefix` such as `^` made `pnpm update` fail in a project with Python dependencies.

`pnpm update` now keeps the precision of a `~=` requirement, so `~=1.0` is updated to `~=1.5` and still allows any 1.x release. With `--latest`, a `==1.*` requirement is updated to `==2.*`.
