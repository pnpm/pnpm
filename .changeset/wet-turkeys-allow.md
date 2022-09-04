---
"@pnpm/default-reporter": patch
"pnpm": patch
---

When an error happens during installation of a subdependency, print some context information in order to be able to locate that subdependency. Print the exact chain of packages that led to the problematic dependency.
