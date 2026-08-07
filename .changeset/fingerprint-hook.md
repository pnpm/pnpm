---
"@pnpm/hooks.pnpmfile": minor
"@pnpm/workspace.state": minor
"@pnpm/deps.status": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added a new pnpmfile hook: `calculateFingerprint`. The hook returns a fingerprint of any external state that the pnpmfile's behavior depends on (for example, the state of a custom package source used by a custom resolver). The fingerprint is recorded in the workspace state file — never the lockfile, so it may be machine-specific — and compared by the up-to-date checks behind `verify-deps-before-run` and `optimistic-repeat-install`. When it changes, `node_modules` is considered outdated even though the lockfile and manifests are unchanged, so a full install (including `shouldRefreshResolution` hooks) runs.

This closes the gap where `shouldRefreshResolution` hooks were skipped by the fast paths ([#10995](https://github.com/pnpm/pnpm/pull/10995)): a pnpmfile can now cheaply signal "my resolution inputs changed" without disabling `optimistic-repeat-install`.

This hook is not available in pacquet, which does not run pnpmfiles.
