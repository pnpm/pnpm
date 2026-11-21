---
"pacquet": patch
---

The peer dependency check now reads the version of an injected workspace package from its `package.json`. `pnpm install` with `strictPeerDependencies: true` and `pnpm peers check` no longer report such a peer as unmet with a `file:` version.
