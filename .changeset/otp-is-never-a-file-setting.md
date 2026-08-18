---
"@pnpm/config.commands": minor
"@pnpm/config.reader": minor
"pacquet": patch
"pnpm": minor
---

`otp` is no longer read from any config file. A one-time password is valid for about thirty seconds, so it describes a single invocation rather than a project or a machine, and a committed `pnpm-workspace.yaml` is the wrong place for a credential. Pass `--otp` or set `PNPM_CONFIG_OTP` instead; a `pnpm-workspace.yaml` or global `config.yaml` that carries `otp` is now ignored with a warning, and `pnpm config set otp` fails with `ERR_PNPM_CONFIG_SET_NOT_A_FILE_SETTING`.

This also closes a way to leak a secret: a repository could otherwise write `otp: ${NPM_TOKEN}` beside its own `registry:` and have the publisher's environment variable sent to a registry of its choosing, in the `npm-otp` header of the first request.
