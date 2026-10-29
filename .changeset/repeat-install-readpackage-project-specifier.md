---
"pacquet": patch
---

A repeat `pnpm install` reports "Already up to date" again when a pnpmfile `readPackage` hook changes the dependencies of a workspace project. Since 12.15.0, every such install ran in full [#17468](https://github.com/pnpm/pnpm/issues/17468).

After an edit to a project's `package.json`, `pnpm install` and `verifyDepsBeforeRun` now run the pnpmfile's `readPackage` hook on the edited manifest before comparing it with the lockfile. They could report the project up to date when the hook rewrote the edited dependency to a specifier the lockfile did not record.
