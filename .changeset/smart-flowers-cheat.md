---
"@pnpm/plugin-commands-store-inspecting": major
"@pnpm/headless": major
"@pnpm/core": major
"@pnpm/cafs-types": major
"@pnpm/store.cafs": major
"@pnpm/worker": major
"pnpm": major
---

Changed the structure of the index files in the store to store side effects cache information more efficiently. In the new version, side effects do not list all the files of the package but just the differences [#8636](https://github.com/pnpm/pnpm/pull/8636).
