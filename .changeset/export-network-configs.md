---
"@pnpm/config.reader": minor
---

Export `getNetworkConfigs`, `getDefaultCreds`, and the `NetworkConfigs` type so consumers can derive a `configByUri` map from a flat npmrc-style auth dict without re-implementing the parsing logic.
