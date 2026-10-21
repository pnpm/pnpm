---
"pacquet": patch
---

`packageImportMethod: copy` now also applies to packages that have lifecycle scripts or patches. pnpm 12 imported those packages with `clone-or-copy` whatever the setting was, so on a filesystem that supports block cloning, such as ReFS or a Dev Drive on Windows, they were still cloned. pnpm 11 already copies them.
