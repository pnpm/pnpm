---
"pacquet": patch
---

Installs fail when a dependency group is not an object or a dependency specifier is not a string. A `readPackage` hook can correct either error before validation.
