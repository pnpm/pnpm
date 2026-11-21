---
"pacquet": minor
---

Deprecation warnings moved into the install summary and only name the packages the install added. A deprecated dependency added by the install is marked `deprecated` on its summary line, with the version that is not deprecated when there is one. Deprecated subdependencies the install added are listed right after the summary. Deprecated packages that were already installed are reported as one count, such as `3 deprecated packages are still installed`.
