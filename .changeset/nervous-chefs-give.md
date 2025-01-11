---
"@pnpm/default-reporter": minor
"@pnpm/build-modules": minor
"pnpm": minor
---

Added a new field "pnpm.ignoredBuiltDependencies" for explicitly listing packages that should not be built. When a package is in the list, pnpm will not print an info message about that package not being built [#8935](https://github.com/pnpm/pnpm/issues/8935).
