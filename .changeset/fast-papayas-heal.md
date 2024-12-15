---
"@pnpm/config": patch
---

Symlink are not supported in the exFAT driver, and installation of dependencies will result in errors. Set the `node-linker` configuration in the exFAT driver to be `hoisted` by default.
