---
"@pnpm/fs.indexed-pkg-importer": patch
---

Fixed cloning files on macOS and Windows when `@pnpm/fs.indexed-pkg-importer` is used outside the pnpm CLI bundle. The package called `require` in an ES module, which threw `require is not defined`.
