---
"pacquet": patch
---

`pnpm publish` now reads `access`, `tag`, `provenance`, and `publishBranch` from `pnpm-workspace.yaml`, from the global `config.yaml`, and from the matching `PNPM_CONFIG_*` environment variables. The `--access`, `--tag`, `--provenance`, and `--publish-branch` flags still win when both are set. `publishBranch` is read from `pnpm-workspace.yaml` only, as it is in pnpm. None of these settings are read from an `.npmrc`.

`pnpm stage` reads the same settings. `--otp` now also reaches `stage approve`, `stage reject`, and the other subcommands that answer a two-factor challenge. It used to reach only `stage publish`.

A configured `access` outranks `publishConfig.access`, so `access: public` at a monorepo root now publishes every package under it as public, including one that sets `publishConfig.access: restricted`. This matches pnpm. Previously only `--access` could do that.

`pnpm publish --no-provenance` now turns provenance off for a single run. It overrides a configured `provenance: true`, and it keeps the OIDC exchange from switching provenance back on. It used to parse and do nothing [#13542](https://github.com/pnpm/pnpm/issues/13542).
