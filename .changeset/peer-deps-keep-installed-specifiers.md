---
"@pnpm/pkg-manifest.utils": patch
"pnpm": patch
---

`pnpm add` and `pnpm update` no longer let a `peerDependencies` entry override the version specifier of the same dependency declared in `dependencies`, `devDependencies`, or `optionalDependencies` when `autoInstallPeers` is enabled. For instance, running `pnpm add react@19.2.7 --save-exact` in a project that also declares `react: ^19.0.0` in `peerDependencies` now saves `19.2.7`, not `^19.2.7`. See pnpm/pnpm#13108.
