---
"pnpm": major
---

Filtering by path is done by globs.

In pnpm v6, in order to pick packages under a certain directory, the following filter was used: `--filter=./apps`

In pnpm v7, a glob should be used: `--filter=./apps/**`
