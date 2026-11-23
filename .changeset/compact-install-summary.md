---
"pnpm": minor
---

The install summary prints an updated dependency on one line, such as `↑ lodash 4.17.20 → 4.18.1`. A downgrade is marked with `↓`. The changed part of the new version is colored the way `pnpm outdated` colors it.

In a workspace, the summary names the projects behind each change only when the changes were made by different projects. A change made by one project shows its package name, such as `in @scope/app`. A change made by several projects shows how many, such as `in 5 projects`.

Deprecated packages, peer dependency issues, and the lockfile's supply-chain verdict are now printed together at the end of the install, above `Done in`. Deprecated packages are listed on one line, such as `✗ 2 packages are deprecated: request@2.88.2 and uuid@3.4.0`.

Dependencies with newer versions are named on one line at the end, with a hint to run `pnpm outdated`. Summary lines no longer mark deprecated packages or end with "(x is available)". "Already up to date" is only printed when the install changed nothing.
