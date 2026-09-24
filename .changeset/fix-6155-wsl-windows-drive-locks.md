---
"@pnpm/fs.graceful-fs": patch
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` in WSL now waits out Windows file locks on a Windows drive such as `/mnt/c`, as it already does on Windows. Before, an antivirus or indexer scan holding a file open could fail the install with `EACCES` [pnpm/pnpm#6155](https://github.com/pnpm/pnpm/issues/6155).
