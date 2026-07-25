---
"pacquet": patch
---

`pnpm install` no longer nests a copy of a dependency that the package also declares as a peer, when the parent already supplies that name. This is what pnpm 11 does with `autoInstallPeers` disabled, and the divergence showed up in large workspaces such as [Astro](https://github.com/withastro/astro) as duplicated peer-suffixed variants in `pnpm-lock.yaml` [#13334](https://github.com/pnpm/pnpm/issues/13334).
