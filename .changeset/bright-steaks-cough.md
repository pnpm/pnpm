---
"@pnpm/store-connection-manager": minor
"@pnpm/package-requester": minor
"@pnpm/plugin-commands-rebuild": minor
"@pnpm/package-store": minor
"@pnpm/config": minor
"pnpm": minor
---

Some registries allow the exact same content to be published under different package names and/or versions. This breaks the validity checks of packages in the store. To avoid errors when verifying the names and versions of such packages in the store, you may now set the `strict-store-pkg-content-check` setting to `false` [#4724](https://github.com/pnpm/pnpm/issues/4724).
