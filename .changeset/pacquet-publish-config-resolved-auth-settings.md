---
"pacquet": patch
---

`pnpm publish` now reads `access`, `tag`, `provenance`, and `publishBranch` from `pnpm-workspace.yaml`, the global `config.yaml`, and the matching `PNPM_CONFIG_*` environment variables, not only from the `--access`, `--tag`, `--provenance`, and `--publish-branch` command-line flags. The flags still take precedence when both are set. `publishBranch` is workspace-only — like pnpm, it is ignored in the global `config.yaml`. None of these settings are read from an `.npmrc`, where pnpm keeps only auth and network keys.

`pnpm stage` picks up the same settings, and `--otp` now also reaches `stage approve`, `stage reject`, and the other subcommands that answer a two-factor challenge, instead of only `stage publish`.

Because a configured `access` outranks the manifest, `access: public` at a monorepo root overrides every package that sets `publishConfig.access: restricted`, as it does in pnpm. Previously only `--access` could do that.

`otp` is deliberately **not** a config-file setting, which is a divergence from pnpm. A one-time password is valid for about thirty seconds, so it describes a single invocation rather than a project or a machine, and a committed `pnpm-workspace.yaml` is the wrong place for a credential. Use `--otp` or `PNPM_CONFIG_OTP`; `pnpm config set otp` now fails with `ERR_PNPM_CONFIG_SET_NOT_A_FILE_SETTING` rather than writing an entry that would be ignored. This also closes a way to leak a secret: a repository could otherwise have written `otp: ${NPM_TOKEN}` beside its own `registry:` and had the publisher's environment variable sent to a registry of its choosing.

`pnpm publish --no-provenance` now turns provenance off for a single run, overriding a configured `provenance: true` and keeping the OIDC exchange from switching it back on. It used to parse but do nothing.
