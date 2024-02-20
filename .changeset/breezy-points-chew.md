---
"@pnpm/plugin-commands-store-inspecting": minor
"@pnpm/plugin-commands-publishing": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/store-connection-manager": minor
"@pnpm/plugin-commands-rebuild": minor
"@pnpm/plugin-commands-env": minor
"@pnpm/client": minor
"@pnpm/core": minor
"@pnpm/types": minor
"@pnpm/config": minor
"@pnpm/fetch": minor
"pnpm": minor
---

Added support for registry-scoped SSL configurations (cert, key, and ca). Three new settings supported: `<registryURL>:certfile`, `<registryURL>:keyfile`, and `<registryURL>:ca`. For instance:

```
//registry.mycomp.com/:certfile=server-cert.pem
//registry.mycomp.com/:keyfile=server-key.pem
//registry.mycomp.com/:cafile=client-cert.pem
```

Related issue: [#7427](https://github.com/pnpm/pnpm/issues/7427).
Related PR: [#7626](https://github.com/pnpm/pnpm/pull/7626).
