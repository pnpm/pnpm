---
"@pnpm/installing.env-installer": patch
"pnpm": patch
"pacquet": patch
---

A config dependency carrying an inline integrity (the `<version>+<integrity>` form, or the object form without a `tarball`) now takes its tarball URL from the registry's packument instead of deriving it from the registry URL, so migrating one costs an extra metadata request. On a registry that serves tarballs from a path pnpm cannot derive, GitLab's group endpoint for one, installing such a config dependency failed with a 404 while the same package installed fine as a regular dependency [#13765](https://github.com/pnpm/pnpm/issues/13765).
