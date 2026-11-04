---
"pacquet": patch
---

`pnpm peers check` warns when a required peer is resolved without a provider declared by the project or its dependency ancestors with `autoInstallPeers: false` and `dedupePeerDependents: true`. These advisory warnings do not change the command's exit code or cause `strictPeerDependencies` failures.
