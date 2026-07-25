---
"pacquet": patch
---

Two `pnpm install` peer-resolution fixes that made large workspaces such as [Astro](https://github.com/withastro/astro) produce a different `pnpm-lock.yaml` than pnpm 11 [#13334](https://github.com/pnpm/pnpm/issues/13334):

- A package that declares the same name in both `dependencies` and `peerDependencies` no longer gets a nested copy of it when the parent already supplies that name, which is what pnpm does with `autoInstallPeers` disabled. The nested copy hid the peer, so the package was recorded without the peer context it resolves in.
- A duplicate peer-suffixed variant that collapses into a larger, compatible one now collapses everywhere it is referenced. A variant kept alive by a single consumer's edge no longer lingers in the lockfile.
