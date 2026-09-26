---
"pacquet": patch
---

`pnpm install` fails with an error that names the dependency, the package and the pnpmfile when a `readPackage` hook sets a dependency range to a value other than a string, such as `undefined`. Delete the property to remove a dependency [#15705](https://github.com/pnpm/pnpm/issues/15705).
