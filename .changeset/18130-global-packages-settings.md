---
"pacquet": minor
---

Settings under `globalPackages` in the global `config.yaml` now apply only to commands that act on global packages, such as `pnpm add -g` and `pnpm update -g`. For example, `globalPackages.trustPolicyExclude` excludes a package from the trust policy for global installs only [#18130](https://github.com/pnpm/pnpm/issues/18130).

`pnpm approve-builds -g` and `pnpm add -g --allow-build` now save build decisions under `globalPackages.allowBuilds`. Build settings in the `pnpm-workspace.yaml` of the global packages directory are deprecated. pnpm still applies them with a warning, and the next approval moves their `allowBuilds` entries to `config.yaml`.
