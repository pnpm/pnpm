---
"pacquet": patch
---

`pnpm install` now fails with an error when a `readPackage` hook returns something other than a package manifest object. A hook that returned a string, number, or array used to install the package with no dependencies and report success [pnpm/pnpm#15730](https://github.com/pnpm/pnpm/issues/15730).
