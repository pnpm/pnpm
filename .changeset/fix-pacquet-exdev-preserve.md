---
"pacquet": patch
---

`pacquet` now preserves `node_modules` when a forced reinstall has to move a package directory across an `EXDEV` filesystem boundary.
