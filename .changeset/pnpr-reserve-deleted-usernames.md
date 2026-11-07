---
"@pnpm/pnpr": patch
---

Accounts and teams now use immutable IDs for permissions and ownership. Renaming preserves an account's grants. Reusing a deleted username or team name no longer inherits its grants. Auth storage and configuration formats have changed; experimental deployments need fresh state.
