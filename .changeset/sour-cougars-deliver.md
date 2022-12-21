---
"@pnpm/build-modules": minor
"pnpm": minor
---

When the hoister node-linker is used, pnpm should not build the same package multiple times during installation. If a package is present at multipe locations because hoisting could not hoist them to a single directory, then the package should only built in one of the locations and copied to the rest.
