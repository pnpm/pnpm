## 1100.2.4

### Patch Changes

- Catalogs now work with entries and catalog names such as `constructor` or `toString`. Pruning unused catalog entries crashed on such names, and a new named catalog called `toString` was silently not written.
