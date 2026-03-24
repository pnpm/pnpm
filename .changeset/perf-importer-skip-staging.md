---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Skip the staging directory when importing packages to a non-existing target directory (the common case for cold installs). This avoids the overhead of creating a temp dir and renaming per package. Falls back to the atomic staging path if the directory already exists or on error.
