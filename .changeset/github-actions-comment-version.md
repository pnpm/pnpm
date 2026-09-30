---
"@pnpm/deps.github-actions": patch
"pnpm": patch
---

Updating a pinned GitHub Action now rewrites the version in its `# vX.Y.Z` comment even when the action name contains the same version text. The action name was changed and the comment kept the old version.
