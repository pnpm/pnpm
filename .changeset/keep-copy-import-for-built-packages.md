---
"pacquet": patch
---

`packageImportMethod: copy` now also applies to packages that have lifecycle scripts or patches. On a filesystem that supports block cloning, such as ReFS or a Dev Drive on Windows, these packages were cloned [#17195](https://github.com/pnpm/pnpm/issues/17195).
