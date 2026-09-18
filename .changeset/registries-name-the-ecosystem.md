---
"pacquet": minor
---

A `registries` entry can now name the ecosystem it serves.

```yaml
registries:
  https://internal.example/simple/:
    ecosystem: pypi
  https://pypi.org/simple/:
    ecosystem: pypi
  https://index.crates.io/:
    ecosystem: cargo
```

`ecosystem` accepts `npm`, `cargo` and `pypi`. An entry that does not name one serves npm, as every entry did before.

An ecosystem with several indexes searches them in the order they are declared. The first index that has a package supplies it, so the one declared last answers what none before it had.

A `registries` entry may not carry credentials. pnpm reads them from `.npmrc`, matched by origin, for a PyPI index as for every other package source.
