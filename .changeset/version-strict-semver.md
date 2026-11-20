---
"pacquet": patch
---

`pnpm version` now rejects a version argument that is not valid semver, such as `1.2.3foo` or `01.2.3`. It used to set a normalized version that differed from the argument.
