---
"@pnpm/hooks.pnpmfile": patch
"pnpm": patch
---

A `readPackage` hook that sets a dependency range to a value other than a string, such as `undefined`, now fails the install with an error that names the dependency, the package, and the pnpmfile. Delete the property to remove a dependency [#5517](https://github.com/pnpm/pnpm/issues/5517).
