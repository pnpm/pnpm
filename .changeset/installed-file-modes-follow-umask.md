---
"@pnpm/bins.linker": patch
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
"pacquet": patch
---

Files imported from the store now follow the umask of the install that writes them. Installing with a umask of `077` no longer leaves imported files readable by the group and others [#3807](https://github.com/pnpm/pnpm/issues/3807).
