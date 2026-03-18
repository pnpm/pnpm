---
'@pnpm/installing.commands': minor
'pnpm': minor
---

Implement `pnpm ci` command for clean installs

This implements the `pnpm ci` (clean-install) command, which is similar to `npm ci`. The command:

- Removes `node_modules` before installation (clean install)
- Installs dependencies from the lockfile with `--frozen-lockfile`
- Fails if the lockfile is missing or out of sync with `package.json`
- Supports workspaces (removes `node_modules` from all workspace projects)

This is useful for CI/CD environments where you want to ensure reproducible builds.

Aliases: `pnpm clean-install`, `pnpm ic`, `pnpm install-clean`

Closes #6100
