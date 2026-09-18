---
"pacquet": minor
---

A `registries` entry can now name the ecosystem it serves.

```yaml
registries:
  https://pypi.org/simple/:
    ecosystem: pypi
    default: true
  https://internal.example/simple/:
    ecosystem: pypi
  https://index.crates.io/:
    ecosystem: cargo
```

`ecosystem` accepts `npm`, `cargo` and `pypi`. An entry that does not name one serves npm, as every entry did before.

`default` marks the index an ecosystem falls back to. The others are searched before it, and it answers what none of them had. Name one once an ecosystem has more than one index. An npm registry is named the default by the `registry` setting.

A `registries` entry may not carry credentials. pnpm reads them from `.npmrc`, matched by origin, for a PyPI index as for every other package source.
