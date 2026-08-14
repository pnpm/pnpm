---
"pacquet": patch
---

`pnpm install` no longer re-resolves dependencies inside a subtree the lockfile pinned when another dependency reaches the same package. Those packages kept their locked versions in `node_modules` while `pnpm-lock.yaml` recorded newer ones, so an install could quietly move a transitive dependency — including across a major version — without anything asking it to.
