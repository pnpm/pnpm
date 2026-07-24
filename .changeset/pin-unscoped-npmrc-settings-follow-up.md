---
"pacquet": patch
---

Closed the remaining gaps in how unscoped per-registry `.npmrc` settings are pinned to the registry their own source file declared:

- An inline `cert=` / `key=` written with `\n` escapes now expands to a real multi-line PEM, matching the URL-scoped `//host/:cert=` spelling.
- `pnpm config get` / `pnpm config list` now report a rescoped credential under the URL-scoped key it was pinned to, instead of the unscoped key it was written as.
- The deprecation warning names the file it read and lists every setting it pinned, including `tokenHelper`.
- A credential with no registry of its own is no longer attached to the resolved default registry, which repository config can move. The same rule now covers the `@pnpm/napi` bindings: the `authHeaderByUri` entry written with an empty (`""`) key is pinned to the `registry` / `registries.default` the host passed alongside it, never to a registry the project's `.npmrc` names.
