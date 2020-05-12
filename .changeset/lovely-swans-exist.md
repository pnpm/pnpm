---
"@pnpm/modules-cleaner": major
"@pnpm/package-requester": major
"@pnpm/package-store": major
"@pnpm/plugin-commands-installation": major
"@pnpm/plugin-commands-store": major
"@pnpm/server": major
"@pnpm/store-connection-manager": minor
"@pnpm/store-controller-types": major
"supi": minor
"@pnpm/types": major
---

Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
