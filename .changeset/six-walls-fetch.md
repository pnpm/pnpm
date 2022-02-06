---
"@pnpm/package-store": patch
---

When checking whether a package is linked from the store, don't fail if the package has no `package.json` file.
