---
"@pnpm/fs.indexed-pkg-importer": minor
"@pnpm/cafs-types": minor
"@pnpm/create-cafs-store": minor
"@pnpm/store-controller-types": minor
---

A new option added to package importer for keeping modules directory: `keepModulesDir`. When this is set to true, if a package already exist at the target location and it has a node_modules directory, then that node_modules directory is moved to the newly imported dependency. This is only needed when node-linker=hoisted is used.
