---
"@pnpm/deps.compliance.audit": patch
"pnpm": patch
---

Fix a `pnpm audit` performance regression on lockfiles that contain dependency cycles. The reachable-vulnerability pruning added in pnpm 11.5.1 only memoized acyclic subtrees, so any node whose subtree touched a cycle — together with all of its ancestors — was recomputed on every query, making the path walk quadratic. Reachability is now computed once per node using Tarjan's strongly-connected-components algorithm, so cyclic graphs are handled in linear time [#12212](https://github.com/pnpm/pnpm/issues/12212).

The audit path walk also no longer recurses, so a deeply nested dependency graph can no longer overflow the call stack, and the install path to each finding is tracked without per-node copying, keeping memory linear in the graph depth.
