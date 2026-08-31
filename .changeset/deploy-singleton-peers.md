---
"@pnpm/releasing.commands": minor
"pnpm": minor
"pacquet": minor
---

`pnpm deploy` no longer requires `injectWorkspacePackages` to be enabled. A linked workspace dependency is rewritten to a `file:` dependency in the dedicated deploy lockfile, and the peer dependencies it declares are bound to the deployed graph's own resolution.

When a peer resolves to more than one version in that graph the binding is ambiguous, and choosing between the candidates is exactly what injecting the package would have decided, so the deploy still fails — now with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER`, which names the package, the peer, and the competing versions, instead of refusing every non-injected workspace up front [#9386](https://github.com/pnpm/pnpm/issues/9386).
