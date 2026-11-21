---
"pacquet": patch
---

A `<pkg>@<version>: false` entry in `allowBuilds` or `permissions` now also blocks `<pkg>@<version>` from a named registry, including when `dangerouslyAllowAllBuilds` is set.
