---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed an injected workspace dependency (`injectWorkspacePackages: true`) incorrectly staying as `file:` instead of deduping back to `link:` when an unrelated, ordinary shared dependency resolved to a peer-suffixed variant for the target project's own copy but not for the injected occurrence. See pnpm/pnpm#10433.
