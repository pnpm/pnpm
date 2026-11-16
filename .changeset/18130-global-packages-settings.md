---
"pacquet": minor
---

Settings under `globalPackagesConfig` in the global `config.yaml` now apply only to commands that act on global packages, such as `pnpm add -g` and `pnpm update -g`. For example, `globalPackagesConfig.trustPolicyExclude` excludes a package from the trust policy for global installs only [#18130](https://github.com/pnpm/pnpm/issues/18130).

`pnpm approve-builds -g` and `pnpm add -g --allow-build` now save build decisions under `globalPackagesConfig.permissions`. pnpm still applies build settings from the `pnpm-workspace.yaml` of the global packages directory, with a deprecation warning. The next approval moves them to `config.yaml`.
