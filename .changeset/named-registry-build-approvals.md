---
"pacquet": patch
---

`allowBuilds` and `permissions` entries now match packages from a named registry the same way as packages from the default registry. A `<pkg>@<version>: false` denial blocks `<pkg>@work:<version>`, and a `<pkg>: true` approval lets it build.
