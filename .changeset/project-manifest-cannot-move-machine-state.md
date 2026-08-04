---
"@pnpm/config.reader": patch
"pnpm": patch
---

A project's `pnpm-workspace.yaml` can no longer decide where pnpm reads and writes state that outlives the project. `configDir` set in a repository redirected the `auth.ini` that `pnpm login` writes the granted token to; `pnpmHomeDir` redirected the directory `pnpm setup` puts on the user's PATH; `bin` redirected where `pnpm install` links its dependencies' bins; and `packageManagerRegistries` redirected where pnpm downloads its own next version from. These settings, along with `dir`, `globalPkgDir`, `npmrcAuthFile`, `rootProjectManifestDir`, `userconfig`, `workspaceDir`, `authConfig`, `configByUri`, and `packageManagerNetworkConfig`, now come from the environment, the global config file, and the command line only, and a project manifest that sets one of them is reported as ignored [#13629](https://github.com/pnpm/pnpm/issues/13629).
