---
"pacquet": minor
---

A `registries` entry can now name the ecosystem it serves, so one setting says where the packages of every ecosystem come from.

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

`default` marks the index an ecosystem resolves from first, where the rest are searched after it. It is needed once an ecosystem has more than one index. An npm registry is named the default by the `registry` setting instead.

A `registries` entry may not carry credentials. pnpm reads them from `.npmrc`, matched by origin, for a PyPI index as for every other package source.
