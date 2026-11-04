---
"pacquet": patch
---

`pnpm dedupe --check` no longer fails when the only difference is the integrity of a locked version that the registry now reports with another algorithm, such as a `sha512` integrity replacing the `sha1` shasum it reported when the lockfile was written [pnpm/pnpm#17692](https://github.com/pnpm/pnpm/issues/17692).
