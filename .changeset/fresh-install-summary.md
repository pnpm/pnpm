---
"pnpm": patch
---

The install summary of a project with no previous install lists only the dependencies the command added or changed. A fresh `pnpm install` of a single project no longer lists every direct dependency, which matches how workspace projects were already reported.

In a workspace, the summary again lists a dependency added to a project whose previous install had no dependencies.
