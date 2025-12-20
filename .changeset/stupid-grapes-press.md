---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Packages that don't have a `package.json` file (like Node.js) should not be reimported from the store on every install. Another file from the package should be checked in order to verify its presence in `node_modules`.
