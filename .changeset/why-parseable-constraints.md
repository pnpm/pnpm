---
"pacquet": minor
---

`pnpm why --parseable` now prints each child package's declared version constraint as `(by <specifier>)` in the path. The Rust dependents renderer exposed through `@pnpm/napi` now prints the same annotations [pnpm/pnpm#13352](https://github.com/pnpm/pnpm/issues/13352).
