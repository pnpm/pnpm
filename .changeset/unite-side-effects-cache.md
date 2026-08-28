---
"@pnpm/building.commands": minor
"@pnpm/config.reader": minor
"@pnpm/types": minor
"@pnpm/engine.runtime.commands": minor
"@pnpm/installing.commands": minor
"pacquet": minor
"pnpm": minor
---

`sideEffectsCache` is now the whole declaration of how a package's build output is reused:

```yaml
sideEffectsCache:
  read: true
  write: true
  remote:
    organization: acme
    packages: ['native-addon']
```

The older spellings still work and mean what they always did: `sideEffectsCache: true` is the shorthand for reading and writing, `sideEffectsCacheReadonly` is reading without writing, and `remoteSideEffectsCache` is the `remote` tier. Where both are set the canonical form wins, and the two spellings of `remote` compose rather than replace — a repository may name the organization under one while the machine supplies the signing key under the other.

The declaration can express one thing the booleans could not: writing without reading, which is what a job that warms a cache it never consumes wants.
