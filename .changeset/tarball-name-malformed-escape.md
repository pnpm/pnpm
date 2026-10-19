---
"pacquet": patch
---

`pnpm add` no longer reads a package name with a malformed percent escape from a registry tarball URL. It installs such a URL as a plain tarball URL.
