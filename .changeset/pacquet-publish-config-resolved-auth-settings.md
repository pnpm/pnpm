---
"pacquet": patch
---

`pnpm publish` now reads `access`, `tag`, `provenance`, `otp`, and `publishBranch` from `pnpm-workspace.yaml`, the global `config.yaml`, and the matching `PNPM_CONFIG_*` environment variables, not only from the `--access`, `--tag`, `--provenance`, `--otp`, and `--publish-branch` command-line flags. The flags still take precedence when both are set, and a configured `access` outranks the manifest's `publishConfig.access`. `publishBranch` is workspace-only — like pnpm, it is ignored in the global `config.yaml`. None of these settings are read from an `.npmrc`, where pnpm keeps only auth and network keys.

`pnpm stage` picks up the same settings: `stage publish` takes all five, and a configured `otp` now also reaches `stage approve`, `stage reject`, and the other subcommands that answer a two-factor challenge.

`pnpm publish --no-provenance` now turns provenance off for a single run, overriding a configured `provenance: true` and keeping the OIDC exchange from switching it back on. It used to parse but do nothing.
