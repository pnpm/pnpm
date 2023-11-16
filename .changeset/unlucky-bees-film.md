---
"@pnpm/npm-resolver": major
"pnpm": minor
---

(IMPORTANT) When the package tarballs aren't hosted on the same domain on which the registry (the server with the package metadata) is, the dependency keys in the lockfile should only contain `/<pkg_name>@<pkg_version`, not `<domain>/<pkg_name>@<pkg_version>`.

This change is a fix to avoid the same package from being added to `node_modules/.pnpm` multiple times. The change to the lockfile is backward compatible, so previous versions of pnpm will work with the fixed lockfile.

We recommend that all team members update pnpm in order to avoid repeated changes in the lockfile.

Related PR: [#7318](https://github.com/pnpm/pnpm/pull/7318).
