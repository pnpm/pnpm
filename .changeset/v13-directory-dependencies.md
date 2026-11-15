---
"pacquet": minor
---

The experimental `v13` lockfile format records a local directory package's declared specifiers in its `dependencies` and `optionalDependencies`, beside the versions they resolved to. A dependency marked `injected` in `dependenciesMeta` has `injected: true`. The format no longer has a `dependenciesMeta` field. Run `pnpm install` to rewrite a `v13` lockfile that an earlier pnpm wrote.
