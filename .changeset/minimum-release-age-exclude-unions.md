---
"pacquet": patch
---

`pnpm install` writes each approved version to `minimumReleaseAgeExclude` as its own `name@version` entry. An existing `name@1.0.0 || 2.0.0` entry still matches those exact versions. Peer-range intersection leaves a version union unresolved when pairing every alternative would allocate the full product [pnpm/pnpm#15867](https://github.com/pnpm/pnpm/issues/15867).
