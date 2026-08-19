---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed an issue where lockfile `packages[].peerDependencies` were rewritten from declared ranges to synthesized exact lower-bound versions on re-resolution when `autoInstallPeers` and `minimumReleaseAge` were active [#13988](https://github.com/pnpm/pnpm/issues/13988).
