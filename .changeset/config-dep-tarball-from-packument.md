---
"@pnpm/installing.env-installer": patch
"pnpm": patch
"pacquet": patch
---

A config dependency declared in the old `<version>+<integrity>` format now takes its tarball URL from the registry's packument instead of deriving it from the registry URL. On a registry that serves tarballs from a path pnpm cannot derive — GitLab's group endpoint, for one — installing such a config dependency failed with a 404 while the same package installed fine as a regular dependency [#13765](https://github.com/pnpm/pnpm/issues/13765).
