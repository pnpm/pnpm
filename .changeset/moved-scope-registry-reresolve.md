---
"@pnpm/config.normalize-registries": patch
"@pnpm/deps.status": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/lockfile.utils": patch
"@pnpm/types": patch
"@pnpm/workspace.state": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` now resolves a scope's packages again when the scope is routed to another registry in `registries`. Before, the lockfile kept the versions and integrity resolved from the old registry, so the install failed when the new registry did not serve a locked version or served different content. pnpm records in the workspace state which registry each scope routed to, so a fresh clone without that record installs the lockfile as recorded. Changing the default registry resolves nothing again, and `pnpm install --frozen-lockfile` is unchanged.
