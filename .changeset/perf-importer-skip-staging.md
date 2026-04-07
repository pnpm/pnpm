---
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/installing.package-requester": patch
"@pnpm/worker": patch
"pnpm": patch
---

Skip the staging directory when importing packages into `node_modules`. This avoids the overhead of creating a temp dir and renaming per package. Falls back to the atomic staging path on error.

Packages that lack a `package.json` now get a synthetic empty one added to the store so that `package.json` can serve as a universal completion marker for the importer.
