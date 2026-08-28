---
"@pnpm/building.commands": minor
"@pnpm/config.reader": minor
"@pnpm/types": minor
"@pnpm/engine.runtime.commands": minor
"@pnpm/installing.commands": minor
"pacquet": minor
"pnpm": minor
---

`sideEffectsCache` now declares the whole of how a package's build output is reused — whether one is restored, whether one is saved, and the remote tier that shares it between machines:

```yaml
sideEffectsCache:
  read: true
  write: true
  remote:
    org: acme
    packages: ['native-addon']
```

`sideEffectsCache: true`, `sideEffectsCacheReadonly`, `remoteSideEffectsCache`, and its `organization` field all keep working. Where a field is set under both spellings the one above wins; where it is set under only one, it is kept.

Two behaviors change, both bringing this CLI in line with what the Rust one already did: `sideEffectsCacheReadonly: true` now blocks writing to the cache, and setting it alongside `sideEffectsCache: false` gives a read-only view rather than switching the cache off entirely. A cache can also be declared write-only now, to populate one the run does not read.
