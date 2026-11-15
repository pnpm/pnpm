---
"pacquet": minor
---

The experimental `v13` lockfile format records a local directory package's declared specifiers in its `dependencies` and `optionalDependencies`, beside the versions they resolved to. A dependency marked `injected` in `dependenciesMeta` has `injected: true`, and the format no longer has a `dependenciesMeta` field. A lockfile written in the `v13` format by an earlier pnpm may need regenerating.
