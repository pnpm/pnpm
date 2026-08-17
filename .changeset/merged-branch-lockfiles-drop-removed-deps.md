---
"@pnpm/installing.context": patch
"pnpm": patch
---

Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a branch lockfile predates the removal of a dependency [#13966](https://github.com/pnpm/pnpm/issues/13966). Merging a branch lockfile can only add entries, so a dependency the main branch has since dropped was reinstated into the merged lockfile. Entries that no project declares anymore are now dropped from the merge.
