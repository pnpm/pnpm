---
"@pnpm/deps.inspection.outdated": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

The `Workspace` column of `pnpm update --interactive` is more informative in two cases. A dependency outdated at the same version in several workspace projects is offered as one choice, since selecting it updates every project — that choice now names all of them instead of only the first. And a workspace project without a `name` is now labelled with its path rather than left blank, so several unnamed projects can be told apart.
