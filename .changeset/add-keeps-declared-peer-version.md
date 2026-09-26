---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---
With `autoInstallPeers`, `pnpm add` and `pnpm remove` in a workspace project keep the locked version of a peer dependency the project declares. In a workspace where another project depended on a different version of that package, the peer could switch to that version [#11225](https://github.com/pnpm/pnpm/issues/11225).
