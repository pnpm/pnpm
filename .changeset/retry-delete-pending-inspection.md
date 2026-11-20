---
"@pnpm/fs.graceful-fs": patch
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
"pacquet": patch
---
Installs in different projects that share a global virtual store no longer fail on Windows with `Access is denied` while repairing the same slot [#15114](https://github.com/pnpm/pnpm/issues/15114).
