---
"pacquet": patch
---

`pnpm change` leaves private packages out when `versioning.includePrivatePackages` is `false`. `pnpm lane` uses that same list. Private packages stay included when the setting is absent [`pnpm/pnpm#13813`](https://github.com/pnpm/pnpm/issues/13813).
