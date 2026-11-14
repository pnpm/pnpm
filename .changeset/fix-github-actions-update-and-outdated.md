---
"@pnpm/deps.github-actions": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed GitHub Actions support in `pnpm update` and `pnpm outdated`. Updating a workflow step with a major or minor version comment such as `# v4` or a prose comment now formats the updated version comment cleanly. Negated action selectors such as `!actions/cache/save` now exclude subpath actions without skipping package updates. `pnpm outdated` now reports every distinct outdated version when several workflows pin the same action at different versions.
