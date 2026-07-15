---
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/patching.commands": patch
"@pnpm/store.create-cafs-store": patch
"pnpm": patch
"pacquet": patch
---

Fixed installs from read-only stores so packages that need patching or build scripts receive a private, owner-writable projection without changing the source store.
